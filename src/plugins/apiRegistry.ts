/**
 * 插件 API 权限元数据(单一事实来源)
 *
 * pluginAPI.ts(主线程,权威校验)与 workerCore.ts(沙箱镜像,预检)两侧共用本表,
 * 避免两份手写的「动作 → 权限」映射漂移 —— 曾经 registerActionButton 两侧都漏权限
 * 就是这类副本导致。新增/调整插件动作时只需改这一处。
 *
 * 取值为 `PluginPermissionType` 的动作需要该权限才能调用;
 * 取值为 `null` 表示该动作不额外设权限(读取/展示类或主侧另有白名单校验,
 * 例如 events.on 由 assertPluginEventSubscriptionAllowed 单独把关)。
 */
import { PluginPermission, type PluginPermissionType } from './pluginTypes'

export const API_ACTION_PERMISSIONS = {
  // ---------- 播放器 ----------
  'player.getState': PluginPermission.PLAYER_READ,
  'player.getLyrics': PluginPermission.PLAYER_READ,
  'player.getCurrentLyricIndex': PluginPermission.PLAYER_READ,
  'player.getCoverPath': PluginPermission.PLAYER_READ,
  'player.play': PluginPermission.PLAYER_CONTROL,
  'player.pause': PluginPermission.PLAYER_CONTROL,
  'player.togglePlay': PluginPermission.PLAYER_CONTROL,
  'player.next': PluginPermission.PLAYER_CONTROL,
  'player.previous': PluginPermission.PLAYER_CONTROL,
  'player.seek': PluginPermission.PLAYER_CONTROL,
  'player.setVolume': PluginPermission.PLAYER_CONTROL,
  'player.setLyrics': PluginPermission.LYRICS_PROVIDER,

  // ---------- 音乐库 ----------
  'library.getPlaylists': PluginPermission.LIBRARY_READ,
  'library.getCurrentPlaylist': PluginPermission.LIBRARY_READ,
  'library.getTracks': PluginPermission.LIBRARY_READ,

  // ---------- 主题 ----------
  'theme.getCurrent': PluginPermission.THEME_READ,
  'theme.getCSSVariable': PluginPermission.THEME_READ,
  'theme.getAllColors': PluginPermission.THEME_READ,
  'theme.setColors': PluginPermission.THEME,

  // ---------- UI 扩展 ----------
  'ui.registerSettingsPanel': PluginPermission.UI_EXTEND,
  'ui.registerMenuItem': PluginPermission.UI_EXTEND,
  'ui.registerPlayerDecorator': PluginPermission.UI_EXTEND,
  'ui.registerActionButton': PluginPermission.UI_EXTEND,
  'ui.unregisterActionButton': PluginPermission.UI_EXTEND,
  'ui.showNotification': null,

  // ---------- 歌词 / 可视化 ----------
  'lyrics.registerProvider': PluginPermission.LYRICS_PROVIDER,
  'visualizer.register': PluginPermission.VISUALIZER,

  // ---------- 命令 / 快捷键 ----------
  'commands.register': PluginPermission.UI_EXTEND,
  'commands.execute': null, // 仅限执行本插件注册的命令,由插件归属校验把关
  'shortcuts.register': PluginPermission.UI_EXTEND,
  'shortcuts.unregister': PluginPermission.UI_EXTEND,

  // ---------- 存储 ----------
  'storage.get': PluginPermission.STORAGE,
  'storage.set': PluginPermission.STORAGE,
  'storage.remove': PluginPermission.STORAGE,
  'storage.getAll': PluginPermission.STORAGE,

  // ---------- 事件(订阅走白名单,emit/off 放开) ----------
  'events.on': null,
  'events.off': null,
  'events.emit': null,

  // ---------- 网络 ----------
  'network.fetch': PluginPermission.NETWORK,

  // ---------- 文件 / 剪贴板 ----------
  'file.saveAs': PluginPermission.FILE_WRITE,
  'file.saveImage': PluginPermission.FILE_WRITE,
  'file.openScreenshotsDirectory': null,
  'clipboard.writeImage': PluginPermission.CLIPBOARD_WRITE,
  'clipboard.writeText': PluginPermission.CLIPBOARD_WRITE,

  // ---------- 工具(纯计算/画布,不设权限) ----------
  'utils.formatTime': null,
  'utils.createCanvas': null,
  'utils.canvasToBlob': null,
  'utils.canvasToDataURL': null,
  'utils.loadImage': null,
  'utils.blobToArrayBuffer': null,
  'utils.dataURLToBlob': null,
  'utils.generateId': null,
} as const satisfies Record<string, PluginPermissionType | null>

export type PluginAction = keyof typeof API_ACTION_PERMISSIONS

/**
 * 查询动作所需权限。未登记的动作视为内部错误(防手抖新增动作忘登记)。
 */
export function permissionForAction(action: string): PluginPermissionType | null {
  const permission = (API_ACTION_PERMISSIONS as Record<string, PluginPermissionType | null>)[action]
  if (permission === undefined) {
    throw new Error(`[apiRegistry] 未知的插件动作: ${action}`)
  }
  return permission
}
