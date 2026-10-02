package com.jdbewl.mercurial_player

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.DocumentsContract
import androidx.activity.ComponentActivity
import androidx.activity.result.ActivityResultLauncher
import androidx.activity.result.contract.ActivityResultContracts
import androidx.documentfile.provider.DocumentFile
import org.json.JSONArray
import org.json.JSONObject

/**
 * SAF（Storage Access Framework）桥：调起目录选择器并持久化 URI 权限、枚举树下的音频文件、
 * 按 content:// URI 打开 fd。fd 经 detachFd 把所有权移交 Rust，由其 File::from_raw_fd 负责关闭。
 */
object SafBridge {
    private const val PREFS_NAME = "saf_access"
    private const val KEY_TREE_URI = "tree_uri"
    /** 每次选择器返回（成功或失败）都递增：前端靠它判定"选择器回来了"，而不是比较 URI
     *  是否变化（重新授权同一个目录时 URI 完全相同，比较 URI 会永远等不到结果）。
     *  成功与否再看 uri / [KEY_PICK_ERROR]：失败时绝不写 KEY_TREE_URI */
    private const val KEY_PICK_VERSION = "pick_version"
    /** 最近一次目录选择失败的原因（空串表示没有失败） */
    private const val KEY_PICK_ERROR = "pick_error"

    private var pickLauncher: ActivityResultLauncher<Intent>? = null
    private var pendingPick = false
    private var appContext: Context? = null

    /** [displayNameForVersion] 的缓存。getPickState 可能来自任意 Rust 线程，
     *  long 的读写不加 volatile 不保证原子；竞争最多多查一次，不会读到脏值 */
    @Volatile
    private var displayNameVersion = -1L

    @Volatile
    private var displayNameValue = ""

    /** 由 MainActivity.onCreate 调用，注册系统目录选择器回调 */
    @JvmStatic
    fun init(activity: ComponentActivity) {
        appContext = activity.applicationContext
        pickLauncher = activity.registerForActivityResult(
            ActivityResultContracts.StartActivityForResult()
        ) { result ->
            pendingPick = false
            if (result.resultCode != Activity.RESULT_OK) return@registerForActivityResult
            val uri = result.data?.data
            if (uri == null) {
                recordPickFailure("系统未返回目录 URI")
                return@registerForActivityResult
            }
            // 只申请系统实际授予的权限位。无条件带上 WRITE 时，只读的提供方会直接抛
            // SecurityException，而旧的 catch 把它吞成日志后仍按"授权成功"保存
            val persistable =
                (result.data?.flags ?: 0) and
                    (Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
            if (persistable == 0) {
                recordPickFailure("系统未授予该目录的持久访问权限")
                return@registerForActivityResult
            }
            try {
                activity.contentResolver.takePersistableUriPermission(uri, persistable)
            } catch (e: Exception) {
                recordPickFailure("持久授权失败: ${e.message}")
                return@registerForActivityResult
            }
            // 持久授权成功才提交成功状态；失败路径见 recordPickFailure
            appContext
                ?.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
                ?.edit()
                ?.putString(KEY_TREE_URI, uri.toString())
                ?.putString(KEY_PICK_ERROR, "")
                ?.putLong(KEY_PICK_VERSION, currentPickVersion() + 1)
                ?.apply()
            android.util.Log.i("SafBridge", "picked tree: $uri (version=${currentPickVersion()})")
        }
    }

    /**
     * 记录一次目录选择失败：递增版本号让前端轮询立刻返回（前端按版本变化判定"选择器已返回"），
     * 原因随 [getPickState] 的 `error` 字段一并返回；不写 KEY_TREE_URI，
     * 避免把只在本会话有效的临时权限记成持久授权。
     */
    private fun recordPickFailure(reason: String) {
        android.util.Log.w("SafBridge", "pick failed: $reason")
        prefs()
            ?.edit()
            ?.putString(KEY_PICK_ERROR, reason)
            ?.putLong(KEY_PICK_VERSION, currentPickVersion() + 1)
            ?.apply()
    }

    /** Rust 命令 saf_pick_directory 调起系统目录选择器 */
    @JvmStatic
    fun requestPick() {
        if (pendingPick) return
        pendingPick = true
        pickLauncher?.launch(Intent(Intent.ACTION_OPEN_DOCUMENT_TREE))
    }

    /** 返回应用数据目录（Rust 侧配置/缓存落盘用，替代只读的 current_exe 目录） */
    @JvmStatic
    fun getAppDataDir(): String =
        appContext?.getDataDir()?.absolutePath ?: ""

    private fun prefs() = appContext?.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)

    private fun currentPickVersion(): Long = prefs()?.getLong(KEY_PICK_VERSION, 0L) ?: 0L

    /** 返回持久化的树 URI（无则 null） */
    @JvmStatic
    fun getSavedTreeUri(): String? = prefs()?.getString(KEY_TREE_URI, null)

    /**
     * 授权状态快照。前端轮询 `version` 判断选择器是否返回：
     * 重新授权同一个目录时 URI 不变，只有 version 会递增；选择失败同样递增 version，
     * 此时 `error` 非空、`uri` 保持原值，前端应立即判定失败而不是等到超时。
     */
    @JvmStatic
    fun getPickState(): String {
        val uri = getSavedTreeUri()
        val version = currentPickVersion()
        return JSONObject()
            .put("uri", uri ?: "")
            .put("version", version)
            .put("error", prefs()?.getString(KEY_PICK_ERROR, "") ?: "")
            .put("displayName", if (uri == null) "" else displayNameForVersion(version))
            .toString()
    }

    /**
     * 显示名按 version 记忆。
     *
     * [getSavedTreeDisplayName] 里的 `DocumentFile.fromTreeUri(...)?.name` 是一次
     * `ContentResolver.query` binder IPC，而前端在等待选择器返回期间每 500ms 轮询一次
     * `getPickState`（最长 3 分钟）—— 只有 version 变了才值得再查一次名字。
     */
    private fun displayNameForVersion(version: Long): String {
        if (version != displayNameVersion) {
            val name = getSavedTreeDisplayName()
            // 先写值再写版本号：反过来的话并发读会拿到"版本已更新、值还是旧的"这一对
            displayNameValue = name
            displayNameVersion = version
        }
        return displayNameValue
    }

    /** 清除持久化的树 URI（同时递增 version，避免前端轮询拿到陈旧状态） */
    @JvmStatic
    fun clearSavedTree() {
        prefs()
            ?.edit()
            ?.remove(KEY_TREE_URI)
            ?.remove(KEY_PICK_ERROR)
            ?.putLong(KEY_PICK_VERSION, currentPickVersion() + 1)
            ?.apply()
        android.util.Log.i("SafBridge", "cleared saved tree")
    }

    private val AUDIO_EXTS = setOf("mp3", "flac", "wav", "ogg", "m4a", "aac")

    /**
     * 递归枚举已授权树下的音频文件，返回
     * `{"files": [{"uri": "content://...", "name": "a.mp3", "folder": "Album", "folderPath": "Album/Sub"}],
     *   "failedDirs": ["Album/Sub"]}`。
     *
     * `failedDirs` 非空表示扫描不完整（个别子目录读取失败），Rust 侧据此拒绝把部分结果当成
     * 完整扫描；为空即完整（含"目录本来就是空的"）。走 `buildChildDocumentsUriUsingTree` + `query`：
     * `DocumentFile.listFiles()` 会对每个子项再发数次查询，深层目录极慢。
     */
    @JvmStatic
    fun listAudioFiles(treeUri: String): String {
        val ctx = appContext
        if (ctx == null) {
            // 桥未初始化属于失败而不是"没有歌"，同样不能给调用方一个完整扫描的假象
            return JSONObject()
                .put("files", JSONArray())
                .put("failedDirs", JSONArray().put("(SAF 桥未初始化)"))
                .toString()
        }
        val tree = Uri.parse(treeUri)
        val out = JSONArray()
        // 读取失败的目录（相对路径或可读名）。查询式枚举在个别子目录失败时不再静默跳过
        val failedDirs = ArrayList<String>()
        val rootId = runCatching { DocumentsContract.getTreeDocumentId(tree) }.getOrNull()
        if (rootId != null) {
            // 树根的解码值与可读名整趟遍历都不变，各算一次；根目录自身的相对路径是空串
            val rootDecoded = decode(rootId)
            val rootName = rootDisplayName(rootId)
            walkQuery(ctx, tree, rootId, "", rootDecoded, rootName, out, failedDirs, 0)
        }
        if (out.length() == 0) {
            // 查询式一个都没拿到：整体回退 DocumentFile（provider 不支持 child documents 查询）
            val root = runCatching { DocumentFile.fromTreeUri(ctx, tree) }.getOrNull()
            if (root != null) walkLegacy(root, out, 0)
            // 回退扫到结果时按完整扫描处理（与旧行为一致）；回退也拿不到则保留失败记录，
            // 让调用方知道"读不到"而不是"目录是空的"
            if (out.length() > 0) failedDirs.clear()
        }
        return JSONObject().put("files", out).put("failedDirs", JSONArray(failedDirs)).toString()
    }

    private fun walkQuery(
        ctx: Context,
        tree: Uri,
        docId: String,
        relDir: String,
        rootDecoded: String,
        rootName: String,
        out: JSONArray,
        failedDirs: MutableList<String>,
        depth: Int,
    ) {
        if (depth > 12) return // 与 Rust 侧 MAX_SCAN_DEPTH 对齐，防 DoS
        val label = relDir.ifEmpty { rootName }
        val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(tree, docId)
        val projection =
            arrayOf(
                DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                DocumentsContract.Document.COLUMN_MIME_TYPE,
            )
        val cursor =
            try {
                ctx.contentResolver.query(childrenUri, projection, null, null, null)
            } catch (e: Exception) {
                android.util.Log.w("SafBridge", "query children failed: $docId", e)
                null
            }
        if (cursor == null) {
            failedDirs.add(label)
            return
        }

        // 先收集子目录 id，避免在 cursor 未关闭时递归嵌套查询
        val subDirs = ArrayList<String>()
        try {
            cursor.use { c ->
                val iId = c.getColumnIndex(DocumentsContract.Document.COLUMN_DOCUMENT_ID)
                val iName = c.getColumnIndex(DocumentsContract.Document.COLUMN_DISPLAY_NAME)
                val iMime = c.getColumnIndex(DocumentsContract.Document.COLUMN_MIME_TYPE)
                if (iId < 0 || iName < 0) return
                while (c.moveToNext()) {
                    val id = c.getString(iId) ?: continue
                    val name = c.getString(iName) ?: continue
                    val mime = if (iMime >= 0) c.getString(iMime) ?: "" else ""
                    when {
                        mime == DocumentsContract.Document.MIME_TYPE_DIR -> subDirs.add(id)
                        // relDir 就是本目录的路径，曲目直接用它：先前每首曲目要为
                        // folder/folderPath 各解一遍 docId 与 rootId（4 次 URLDecoder.decode，
                        // 其中 2 次在解同一个不变的 rootId），万曲规模就是几万趟白活
                        isAudioName(name) ->
                            out.put(
                                JSONObject()
                                    .put("uri", DocumentsContract.buildDocumentUriUsingTree(tree, id).toString())
                                    .put("name", name)
                                    .put("folder", folderDisplay(relDir, rootName))
                                    .put("folderPath", relDir),
                            )
                    }
                }
            }
        } catch (e: Exception) {
            android.util.Log.w("SafBridge", "walk children failed: $docId", e)
            failedDirs.add(label)
            return
        }
        for (sub in subDirs) {
            walkQuery(
                ctx,
                tree,
                sub,
                dirRelativePath(sub, rootDecoded),
                rootDecoded,
                rootName,
                out,
                failedDirs,
                depth + 1,
            )
        }
    }

    /** 回退路径：DocumentFile 递归（仅当查询式枚举拿不到结果时使用） */
    private fun walkLegacy(dir: DocumentFile, out: JSONArray, depth: Int) {
        if (depth > 12) return
        val children = dir.listFiles() ?: return
        for (child in children) {
            if (child.isDirectory) {
                walkLegacy(child, out, depth + 1)
            } else if (child.isFile) {
                val name = child.name ?: continue
                if (!isAudioName(name)) continue
                val id = runCatching { DocumentsContract.getDocumentId(child.uri) }.getOrNull() ?: ""
                out.put(
                    JSONObject()
                        .put("uri", child.uri.toString())
                        .put("name", name)
                        // parentFile 对树根下的文件会返回树根文档（document id），不能直接用
                        .put("folder", child.parentFile?.name ?: "")
                        .put("folderPath", id.substringBeforeLast('/').substringAfter(':', "")),
                )
            }
        }
    }

    private fun isAudioName(name: String): Boolean {
        val ext = name.substringAfterLast('.', "").lowercase()
        return ext in AUDIO_EXTS
    }

    private fun decode(value: String): String =
        runCatching { java.net.URLDecoder.decode(value, "UTF-8") }.getOrDefault(value)

    /**
     * 目录文档 id 相对树根的解码路径，根目录返回空串。
     * 每个目录只算一次；曲目直接从 [walkQuery] 拿到现成的路径。
     */
    private fun dirRelativePath(docId: String, rootDecoded: String): String {
        val d = decode(docId)
        val rel = if (d.startsWith(rootDecoded)) d.substring(rootDecoded.length) else d
        return rel.trimStart('/')
    }

    /** 目录显示名：根目录用树根的可读名（如 primary:Music → Music） */
    private fun folderDisplay(relDir: String, rootName: String): String =
        if (relDir.isEmpty()) rootName else relDir.substringAfterLast('/').ifEmpty { rootName }

    /** 树根文档的可读名：primary:Music → Music */
    private fun rootDisplayName(rootId: String): String {
        val r = decode(rootId)
        val afterColon = r.substringAfterLast(':', "")
        val candidate = if (afterColon.isNotEmpty()) afterColon else r.substringAfterLast('/')
        return candidate.ifEmpty { r }
    }

    /** 已保存树的可读显示名（供 UI 展示，避免把 content:// URI 直接显示给用户） */
    @JvmStatic
    fun getSavedTreeDisplayName(): String {
        val ctx = appContext ?: return ""
        val uri = getSavedTreeUri() ?: return ""
        val parsed = Uri.parse(uri)
        runCatching { DocumentFile.fromTreeUri(ctx, parsed)?.name }
            .getOrNull()
            ?.takeIf { it.isNotEmpty() }
            ?.let { return it }
        val rootId = runCatching { DocumentsContract.getTreeDocumentId(parsed) }.getOrNull()
        return if (rootId != null) rootDisplayName(rootId) else ""
    }

    /**
     * 打开 content:// URI 并返回 detached fd（-1 表示失败）。
     * 所有权移交给调用方，Rust 侧用 File::from_raw_fd 包装后负责关闭。
     */
    @JvmStatic
    fun openFileFd(uri: String): Int {
        val ctx = appContext ?: return -1
        return try {
            val pfd = ctx.contentResolver.openFileDescriptor(Uri.parse(uri), "r") ?: return -1
            val fd = pfd.detachFd()
            pfd.close()
            fd
        } catch (e: Exception) {
            android.util.Log.w("SafBridge", "openFileFd failed for $uri", e)
            -1
        }
    }

    /**
     * 打开 content:// URI 用于**写入**并返回 detached fd（-1 表示失败），所有权移交 Rust。
     * 模式用 `"wt"`（写 + 截断）而不是 `"w"`：保存对话框允许挑已存在的文件，`"w"` 不截断会留下旧文件尾巴。
     */
    @JvmStatic
    fun openOutputFd(uri: String): Int {
        val ctx = appContext ?: return -1
        return try {
            val pfd = ctx.contentResolver.openFileDescriptor(Uri.parse(uri), "wt") ?: return -1
            val fd = pfd.detachFd()
            pfd.close()
            fd
        } catch (e: Exception) {
            android.util.Log.w("SafBridge", "openOutputFd failed for $uri", e)
            -1
        }
    }

    /**
     * 查询 content URI 的显示名（拿不到时返回空串）：URI 本身看不出文件名
     * （下载提供者给的是 `.../document/msf%3A1000000021` 这种），只能回查 provider。
     * Rust 侧用它守住「封面只能写进图片文件」这条扩展名校验。
     */
    @JvmStatic
    fun getDisplayName(uri: String): String {
        val ctx = appContext ?: return ""
        val parsed = Uri.parse(uri)
        val fromProvider =
            runCatching {
                ctx.contentResolver
                    .query(
                        parsed,
                        arrayOf(DocumentsContract.Document.COLUMN_DISPLAY_NAME),
                        null,
                        null,
                        null,
                    )
                    ?.use { c -> if (c.moveToFirst()) c.getString(0) else null }
            }
            .getOrNull()
        if (!fromProvider.isNullOrEmpty()) return fromProvider
        return runCatching { DocumentFile.fromSingleUri(ctx, parsed)?.name }.getOrNull() ?: ""
    }
}