/**
 * 权限元数据的单一事实来源:pluginAPI.ts 的权威校验与 workerCore.ts 的沙箱预检共用本表,避免两份手写映射漂移
 *
 * 值为调用该动作所需的权限;null 表示不额外设限(读取类动作,或由主侧白名单另行把关)
 */
import { PluginPermission, type PluginPermissionType } from './pluginTypes'

const API_ACTION_PERMISSIONS = {
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

  'library.getPlaylists': PluginPermission.LIBRARY_READ,
  'library.getCurrentPlaylist': PluginPermission.LIBRARY_READ,
  'library.getTracks': PluginPermission.LIBRARY_READ,

  'theme.getCurrent': PluginPermission.THEME_READ,
  'theme.getCSSVariable': PluginPermission.THEME_READ,
  'theme.getAllColors': PluginPermission.THEME_READ,
  'theme.setColors': PluginPermission.THEME,

  'ui.registerSettingsPanel': PluginPermission.UI_EXTEND,
  'ui.registerMenuItem': PluginPermission.UI_EXTEND,
  'ui.registerPlayerDecorator': PluginPermission.UI_EXTEND,
  'ui.registerActionButton': PluginPermission.UI_EXTEND,
  'ui.unregisterActionButton': PluginPermission.UI_EXTEND,
  'ui.showNotification': null,

  'lyrics.registerProvider': PluginPermission.LYRICS_PROVIDER,
  'visualizer.register': PluginPermission.VISUALIZER,

  'commands.register': PluginPermission.UI_EXTEND,
  'commands.execute': null, // 仅限执行本插件注册的命令,由插件归属校验把关
  'shortcuts.register': PluginPermission.UI_EXTEND,
  'shortcuts.unregister': PluginPermission.UI_EXTEND,

  'storage.get': PluginPermission.STORAGE,
  'storage.set': PluginPermission.STORAGE,
  'storage.remove': PluginPermission.STORAGE,
  'storage.getAll': PluginPermission.STORAGE,

  // 事件订阅走 pluginTypes 的白名单校验,emit/off 不设权限
  'events.on': null,
  'events.off': null,
  'events.emit': null,

  'network.fetch': PluginPermission.NETWORK,

  'file.saveAs': PluginPermission.FILE_WRITE,
  'file.saveImage': PluginPermission.FILE_WRITE,
  'file.openScreenshotsDirectory': null,
  'clipboard.writeImage': PluginPermission.CLIPBOARD_WRITE,
  'clipboard.writeText': PluginPermission.CLIPBOARD_WRITE,

  // 工具动作是纯计算/画布操作,无副作用,不设权限
  'utils.formatTime': null,
  'utils.createCanvas': null,
  'utils.canvasToBlob': null,
  'utils.canvasToDataURL': null,
  'utils.loadImage': null,
  'utils.blobToArrayBuffer': null,
  'utils.dataURLToBlob': null,
  'utils.generateId': null,
} as const satisfies Record<string, PluginPermissionType | null>

/** 查询动作所需权限;动作未登记即抛错,防止新增动作漏登记 */
export function permissionForAction(action: string): PluginPermissionType | null {
  const permission = (API_ACTION_PERMISSIONS as Record<string, PluginPermissionType | null>)[action]
  if (permission === undefined) {
    throw new Error(`[apiRegistry] 未知的插件动作: ${action}`)
  }
  return permission
}
