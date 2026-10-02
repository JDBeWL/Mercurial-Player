/**
 * 权限元数据单一事实来源：pluginAPI.ts 权威校验与 workerCore.ts 沙箱预检共用本表，
 * 避免两份手写映射漂移——registerActionButton 曾两侧都漏权限。
 * 值为权限表示调用需该权限；null 表示不额外设限（读取类，或由主侧白名单另行把关）。
 */
import { PluginPermission, type PluginPermissionType } from './pluginTypes'

const API_ACTION_PERMISSIONS = {
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
