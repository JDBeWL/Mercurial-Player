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
 * SAF（Storage Access Framework）桥。
 *
 * Rust 侧 cpal/lofty/symphonia 需要真实的本地 fd 才能读取媒体文件，
 * Android 分区存储下绝对路径不可用，因此通过本桥：
 *  1. 调起系统目录选择器（ACTION_OPEN_DOCUMENT_TREE）并持久化 URI 权限；
 *  2. 递归枚举已授权树下的音频文件（返回 uri+名字+所在目录名）；
 *  3. 按 content:// URI 打开文件描述符（detachFd 后所有权移交 Rust，由 Rust 的
 *     File::from_raw_fd 包装，fd 生命周期由 Rust 侧 File 管理）。
 */
object SafBridge {
    private const val PREFS_NAME = "saf_access"
    private const val KEY_TREE_URI = "tree_uri"
    /** 每次成功授权递增：前端靠它判定"选择器回来了"，而不是比较 URI 是否变化
     *  （重新授权同一个目录时 URI 完全相同，比较 URI 会永远等不到结果） */
    private const val KEY_PICK_VERSION = "pick_version"

    private var pickLauncher: ActivityResultLauncher<Intent>? = null
    private var pendingPick = false
    private var appContext: Context? = null

    /** 由 MainActivity.onCreate 调用，注册系统目录选择器回调 */
    @JvmStatic
    fun init(activity: ComponentActivity) {
        appContext = activity.applicationContext
        pickLauncher = activity.registerForActivityResult(
            ActivityResultContracts.StartActivityForResult()
        ) { result ->
            pendingPick = false
            if (result.resultCode == Activity.RESULT_OK) {
                val uri = result.data?.data
                if (uri != null) {
                    try {
                        activity.contentResolver.takePersistableUriPermission(
                            uri,
                            Intent.FLAG_GRANT_READ_URI_PERMISSION or
                                Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                        )
                    } catch (e: Exception) {
                        android.util.Log.w("SafBridge", "takePersistableUriPermission failed", e)
                    }
                    val prefs = appContext?.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
                    prefs
                        ?.edit()
                        ?.putString(KEY_TREE_URI, uri.toString())
                        ?.putLong(KEY_PICK_VERSION, currentPickVersion() + 1)
                        ?.apply()
                    android.util.Log.i("SafBridge", "picked tree: $uri (version=${currentPickVersion()})")
                }
            }
        }
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
     * 重新授权同一个目录时 URI 不变，只有 version 会递增。
     */
    @JvmStatic
    fun getPickState(): String {
        val uri = getSavedTreeUri()
        return JSONObject()
            .put("uri", uri ?: "")
            .put("version", currentPickVersion())
            .put("displayName", if (uri == null) "" else getSavedTreeDisplayName())
            .toString()
    }

    /** 清除持久化的树 URI（同时递增 version，避免前端轮询拿到陈旧状态） */
    @JvmStatic
    fun clearSavedTree() {
        prefs()
            ?.edit()
            ?.remove(KEY_TREE_URI)
            ?.putLong(KEY_PICK_VERSION, currentPickVersion() + 1)
            ?.apply()
        android.util.Log.i("SafBridge", "cleared saved tree")
    }

    private val AUDIO_EXTS = setOf("mp3", "flac", "wav", "ogg", "m4a", "aac")

    /**
     * 递归枚举已授权树下的音频文件。
     *
     * 实现要点：
     * - 走 `DocumentsContract.buildChildDocumentsUriUsingTree` + `ContentResolver.query`，
     *   一个目录一次查询即可拿到全部子项；原先的 `DocumentFile.listFiles()` 会对每个
     *   子项的 `exists()/isDirectory/name/parentFile` 各自再发一次查询，深层目录极慢；
     * - `folder` 取**相对树根**的父目录显示名（原实现取 `parentFile.name`，对树根下的
     *   文件会返回 document id 本身，如 `primary%3AMusic`）；
     * - 个别 provider 不支持 child documents 查询时回退到 DocumentFile 递归。
     *
     * @return JSON 数组，元素形如
     *   {"uri": "content://...", "name": "a.mp3", "folder": "Album", "folderPath": "Album/Sub"}
     */
    @JvmStatic
    fun listAudioFiles(treeUri: String): String {
        val ctx = appContext ?: return "[]"
        val tree = Uri.parse(treeUri)
        val out = JSONArray()
        val rootId = runCatching { DocumentsContract.getTreeDocumentId(tree) }.getOrNull()
        if (rootId != null) {
            runCatching { walkQuery(ctx, tree, rootId, rootId, out, 0) }
                .onFailure { e -> android.util.Log.w("SafBridge", "query walk failed", e) }
        }
        if (out.length() == 0) {
            // 回退：provider 不支持 child documents 查询
            val root = DocumentFile.fromTreeUri(ctx, tree)
            if (root != null) walkLegacy(root, out, 0)
        }
        return out.toString()
    }

    private fun walkQuery(
        ctx: Context,
        tree: Uri,
        docId: String,
        rootId: String,
        out: JSONArray,
        depth: Int,
    ) {
        if (depth > 12) return // 与 Rust 侧 MAX_SCAN_DEPTH 对齐，防 DoS
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
            } ?: return

        // 先收集子目录 id，避免在 cursor 未关闭时递归嵌套查询
        val subDirs = ArrayList<String>()
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
                    isAudioName(name) ->
                        out.put(
                            JSONObject()
                                .put("uri", DocumentsContract.buildDocumentUriUsingTree(tree, id).toString())
                                .put("name", name)
                                .put("folder", folderDisplay(id, rootId))
                                .put("folderPath", relativeDirPath(id, rootId)),
                        )
                }
            }
        }
        for (sub in subDirs) {
            walkQuery(ctx, tree, sub, rootId, out, depth + 1)
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

    /** 文档 id 相对树根的相对目录路径（不含文件名），根目录返回空串 */
    private fun relativeDirPath(docId: String, rootId: String): String {
        val d = decode(docId)
        val r = decode(rootId)
        val rel = if (d.startsWith(r)) d.substring(r.length) else d
        val trimmed = rel.trimStart('/')
        val idx = trimmed.lastIndexOf('/')
        return if (idx > 0) trimmed.substring(0, idx) else ""
    }

    /** 父目录显示名：根目录用树根的可读名（如 primary:Music → Music） */
    private fun folderDisplay(docId: String, rootId: String): String {
        val path = relativeDirPath(docId, rootId)
        if (path.isEmpty()) return rootDisplayName(rootId)
        return path.substringAfterLast('/').ifEmpty { rootDisplayName(rootId) }
    }

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
     * 打开 content:// URI 用于**写入**并返回 detached fd（-1 表示失败）。
     *
     * 模式用 `"wt"`（写 + 截断）而不是 `"w"`：系统保存对话框允许用户挑一个已存在的
     * 文件，`"w"` 不截断，新封面比旧文件短时会留下旧文件的尾巴。
     *
     * 由「提取封面」调用 —— 桌面端写的是保存对话框给的本地路径，安卓端拿到的是
     * content URI，必须走 ContentResolver 拿 fd，直接按路径 fs::write 会失败。
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
     * 查询 content URI 的显示名（拿不到时返回空串）。
     *
     * 保存对话框返回的是 document URI，**URI 本身看不出文件名**
     * （下载提供者给的是 `.../document/msf%3A1000000021` 这种），所以只能回查。
     * Rust 侧用它守住"只往图片文件写封面"这条校验 —— 顺着路径分支的扩展名白名单而来。
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