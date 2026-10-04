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
    /** 每次选择器返回（成功或失败）都递增：前端靠它判定"选择器回来了"，而非比较 URI 是否变化
     *  （重新授权同一目录时 URI 不变）。写入用 commit 而非 apply——这条记录是前端轮询的唯一
     *  判据，异步落盘在对话框期间进程被杀就会丢，前端会空转到超时。 */
    private const val KEY_PICK_VERSION = "pick_version"
    /** 最近一次目录选择失败的原因（空串表示没有失败） */
    private const val KEY_PICK_ERROR = "pick_error"

    /** 用户主动取消时的 [KEY_PICK_ERROR] 文案：前端据此立刻收尾，不必等轮询超时 */
    private const val PICK_CANCELLED = "用户取消了目录选择"

    /**
     * SAF 树扫描的深度上限（防 DoS / 防环状结构）。与桌面端 `media/filesystem.rs` 的
     * `MAX_SCAN_DEPTH` 无关，此处独断；超限会记入 `failedDirs` 让本次扫描报"不完整"，
     * 因此刻意保留更宽松的 12，收紧只会让更多库导入失败。
     */
    private const val MAX_SCAN_DEPTH = 12

    /**
     * 一次目录选择的兜底超时（毫秒），与前端轮询上限一致。选择器在 Activity 重建、进程被回收时
     * 可能永不回调，而 [pendingPick] 卡在 true 就再也调不起选择器）。
     */
    private const val PICK_TIMEOUT_MS = 3 * 60 * 1000L

    private var pickLauncher: ActivityResultLauncher<Intent>? = null

    /** 是否有一次目录选择正在进行。Rust 会从任意线程调 [requestPick]，故 volatile */
    @Volatile
    private var pendingPick = false

    /** 当前这次选择发起的时刻（`SystemClock.elapsedRealtime()`），用于 [PICK_TIMEOUT_MS] 兜底 */
    @Volatile
    private var pendingPickStartedAt = 0L

    /** 由 [init] 在主线程写、[requestPick] 读 */
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
        // Activity 重建后上一次的在途选择已经作废，它的回调挂在旧实例上，必须复位，
        // 否则 pendingPick 会永久卡住，之后再也调不起选择器
        pendingPick = false
        pendingPickStartedAt = 0L
        pickLauncher = activity.registerForActivityResult(
            ActivityResultContracts.StartActivityForResult()
        ) { result ->
            pendingPick = false
            if (result.resultCode != Activity.RESULT_OK) {
                // 取消是最常见的"返回"路径，同样必须递增 version：前端靠 version 变化判定
                // 选择器已返回（见 KEY_PICK_VERSION 注释），不递增就要空转到 3 分钟超时
                recordPickFailure(PICK_CANCELLED)
                return@registerForActivityResult
            }
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
                ?.commit()
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
            ?.commit()
    }

    /** Rust 命令 saf_pick_directory 调起系统目录选择器 */
    @JvmStatic
    fun requestPick() {
        if (pendingPick && !pickTimedOut()) return
        val launcher = pickLauncher
        if (launcher == null) {
            // init 没跑到（Activity 尚未创建）时以前直接 return，前端于是空转到超时。
            // 这里必须回报一次失败：前端靠 version 变化判定"选择器已返回"
            pendingPick = false
            recordPickFailure("目录选择器未就绪（应用尚未初始化）")
            return
        }
        pendingPick = true
        pendingPickStartedAt = android.os.SystemClock.elapsedRealtime()
        try {
            launcher.launch(Intent(Intent.ACTION_OPEN_DOCUMENT_TREE))
        } catch (e: Exception) {
            // 重复 launch / Activity 状态异常
            pendingPick = false
            recordPickFailure("调起目录选择器失败: ${e.message}")
        }
    }

    /** [pendingPick] 是否已超过 [PICK_TIMEOUT_MS]（选择器回调永远不来的兜底） */
    private fun pickTimedOut(): Boolean {
        val startedAt = pendingPickStartedAt
        return startedAt > 0L &&
            android.os.SystemClock.elapsedRealtime() - startedAt > PICK_TIMEOUT_MS
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
     * 某个树 URI 是否仍持有持久读授权。持久授权会被系统回收（撤销、清数据、提供方被卸载），
     * 之后用旧 URI 查询/开 fd 只抛 SecurityException；`persistedUriPermissions` 是唯一可靠判据。
     * 判据必须落在传入的 URI 上而非"已保存的那棵树"——配置里可能有多个 SAF 目录。
     */
    @JvmStatic
    fun isTreePermissionValid(treeUri: String): Boolean {
        if (treeUri.isBlank()) return false
        val resolver = appContext?.contentResolver ?: return false
        val target = Uri.parse(treeUri)
        return runCatching {
            resolver.persistedUriPermissions.any { it.uri == target && it.isReadPermission }
        }.getOrElse { e ->
            android.util.Log.w("SafBridge", "查询持久授权失败: ${e.message}")
            false
        }
    }

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

    /**
     * 移除某个 SAF 目录时由 Rust 调用：释放该树 URI 的持久授权。只删 SharedPreferences 会把
     * grant 永久留在系统里，反复增删累积到上限后系统会拒绝新授权请求；若清掉的正是
     * "当前记住的那棵树"，一并清除记录并递增 version。
     */
    @JvmStatic
    fun clearSavedTree(treeUri: String) {
        releasePersistableTreePermission(treeUri)
        if (treeUri.isBlank() || treeUri == getSavedTreeUri()) {
            prefs()
                ?.edit()
                ?.remove(KEY_TREE_URI)
                ?.remove(KEY_PICK_ERROR)
                ?.putLong(KEY_PICK_VERSION, currentPickVersion() + 1)
                ?.commit()
        }
        android.util.Log.i("SafBridge", "cleared saved tree: $treeUri")
    }

    /** 释放指定树 URI 的持久授权 */
    private fun releasePersistableTreePermission(treeUri: String) {
        if (treeUri.isBlank()) return
        val resolver = appContext?.contentResolver ?: return
        val target = Uri.parse(treeUri)
        runCatching {
            val flags =
                resolver.persistedUriPermissions
                    .firstOrNull { it.uri == target }
                    ?.let { p ->
                        (if (p.isReadPermission) Intent.FLAG_GRANT_READ_URI_PERMISSION else 0) or
                            (if (p.isWritePermission) Intent.FLAG_GRANT_WRITE_URI_PERMISSION else 0)
                    }
                    ?: 0
            if (flags != 0) {
                resolver.releasePersistableUriPermission(target, flags)
                android.util.Log.i("SafBridge", "released persistable permission: $target")
            }
        }.onFailure { e ->
            android.util.Log.w("SafBridge", "释放持久授权失败: ${e.message}")
        }
    }

    /**
     * 扫描时认可的音频扩展名。必须与 Rust 的 `AUDIO_EXTENSIONS` 及前端 `FileUtils.isAudioFile`
     * 是同一份集合，否则同一批文件经本地目录与 SAF 会扫出不同结果
     */
    private val AUDIO_EXTS =
        setOf("mp3", "flac", "wav", "ogg", "m4a", "aac", "aiff", "aif", "caf")

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
        // 授权被系统回收后，每个子目录都会各自抛一次 SecurityException；
        // 这里先判一次，直接给出"需要重新授权"这一条可操作的失败
        if (!isTreePermissionValid(treeUri)) {
            return JSONObject()
                .put("files", JSONArray())
                .put("failedDirs", JSONArray().put("(目录授权已失效，请重新添加该目录)"))
                .toString()
        }
        val rootId = runCatching { DocumentsContract.getTreeDocumentId(tree) }.getOrNull()
        if (rootId == null) {
            failedDirs.add("(无法解析树根文档)")
        } else {
            // 树根的解码值与可读名整趟遍历都不变，各算一次；根目录自身的相对路径是空串
            val rootDecoded = decode(rootId)
            val rootName = rootDisplayName(rootId)
            // 返回值 = provider 是否**明确拒绝**了子文档查询（IllegalArgumentException）。
            // 只有这种情况才值得回退 DocumentFile；"目录里本来就没歌"不再触发一次全量慢速递归
            val childQueryUnsupported =
                walkQuery(ctx, tree, rootId, "", rootDecoded, rootName, out, failedDirs, 0)
            if (childQueryUnsupported) {
                android.util.Log.i("SafBridge", "provider 不支持子文档查询，回退 DocumentFile 递归")
                val root = runCatching { DocumentFile.fromTreeUri(ctx, tree) }.getOrNull()
                if (root != null) {
                    walkLegacy(root, out, 0)
                    // 回退路径跑完即视为完整扫描（与旧行为一致）；跑不起来则保留 walkQuery
                    // 记下的失败，让调用方知道"读不到"而不是"目录是空的"
                    failedDirs.clear()
                }
            }
        }
        return JSONObject().put("files", out).put("failedDirs", JSONArray(failedDirs)).toString()
    }

    /**
     * 查询式递归枚举。返回 `true` 表示 provider **明确不支持**子文档查询
     * （`IllegalArgumentException`，如部分老式 DocumentsProvider），调用方据此回退 DocumentFile。
     * 权限、IO 等真实错误仍记为 [failedDirs] 并返回 `false`。
     */
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
    ): Boolean {
        val label = relDir.ifEmpty { rootName }
        if (depth > MAX_SCAN_DEPTH) {
            // 静默丢弃整棵子树会让"部分歌曲消失"被当成用户删了歌，必须记进 failedDirs，
            // 由 Rust 侧据此拒绝把这次扫描当完整结果
            android.util.Log.w("SafBridge", "目录深度超过 $MAX_SCAN_DEPTH，跳过: $label")
            failedDirs.add(label)
            return false
        }
        val childrenUri = DocumentsContract.buildChildDocumentsUriUsingTree(tree, docId)
        val projection =
            arrayOf(
                DocumentsContract.Document.COLUMN_DOCUMENT_ID,
                DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                DocumentsContract.Document.COLUMN_MIME_TYPE,
            )
        var unsupported = false
        val cursor =
            try {
                ctx.contentResolver.query(childrenUri, projection, null, null, null)
            } catch (e: IllegalArgumentException) {
                // provider 不认这个 URI 形状 = 不支持子文档查询，不是失败
                android.util.Log.i("SafBridge", "provider 不支持子文档查询: $docId")
                unsupported = true
                null
            } catch (e: Exception) {
                android.util.Log.w("SafBridge", "query children failed: $docId", e)
                null
            }
        if (cursor == null) {
            if (!unsupported) failedDirs.add(label)
            return unsupported
        }

        // 先收集子目录 id，避免在 cursor 未关闭时递归嵌套查询
        val subDirs = ArrayList<String>()
        try {
            cursor.use { c ->
                val iId = c.getColumnIndex(DocumentsContract.Document.COLUMN_DOCUMENT_ID)
                val iName = c.getColumnIndex(DocumentsContract.Document.COLUMN_DISPLAY_NAME)
                val iMime = c.getColumnIndex(DocumentsContract.Document.COLUMN_MIME_TYPE)
                if (iId < 0 || iName < 0) {
                    // return@use 而非 return：非局部返回会连后面的子目录递归一起跳过，
                    // 而这里只是本目录的列缺失，必须记进失败列表而不是静默丢一棵子树
                    android.util.Log.w("SafBridge", "查询结果缺少必需列: $docId")
                    failedDirs.add(label)
                    return@use
                }
                while (c.moveToNext()) {
                    val id = c.getString(iId) ?: continue
                    val name = c.getString(iName) ?: continue
                    val mime = if (iMime >= 0) c.getString(iMime) ?: "" else ""
                    when {
                        mime == DocumentsContract.Document.MIME_TYPE_DIR -> subDirs.add(id)
                        // relDir 就是本目录的路径，曲目直接用它：先前每首曲目都要为
                        // folder/folderPath 各解一遍 docId 与 rootId（4 次百分号解码，
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
            return unsupported
        }
        for (sub in subDirs) {
            unsupported =
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
                ) || unsupported
        }
        return unsupported
    }

    /** 回退路径：DocumentFile 递归（仅当查询式枚举明确不被 provider 支持时使用） */
    private fun walkLegacy(dir: DocumentFile, out: JSONArray, depth: Int) {
        if (depth > MAX_SCAN_DEPTH) return
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

    /**
     * 百分号解码 document id（`primary%3AMusic%2FSong.mp3` → `primary:Music/Song.mp3`）。
     * 用 [Uri.decode] 而非 `URLDecoder.decode`：后者会把 `+` 解成空格，而 Rust 侧
     * `saf.rs::percent_decode` 原样保留 `+`，同一目录在两处会得出不同相对路径。
     */
    private fun decode(value: String): String =
        runCatching { Uri.decode(value) }.getOrDefault(value)

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
                        arrayOf(android.provider.OpenableColumns.DISPLAY_NAME),
                        null,
                        null,
                        null,
                    )
                    ?.use { c ->
                        // 必须按列名取下标，不能写 getString(0)：投影顺序不由调用方保证，
                        // provider 可能（且确实会）调整返回列的顺序，取到别的字段就会把
                        // 一个非文件名当成文件名交给 Rust 做扩展名白名单校验
                        val index =
                            c.getColumnIndex(android.provider.OpenableColumns.DISPLAY_NAME)
                        if (index >= 0 && c.moveToFirst()) c.getString(index) else null
                    }
            }
            .getOrNull()
        if (!fromProvider.isNullOrEmpty()) return fromProvider
        // provider 没给列名时再退到 DocumentFile
        return runCatching { DocumentFile.fromSingleUri(ctx, parsed)?.name }.getOrNull() ?: ""
    }
}