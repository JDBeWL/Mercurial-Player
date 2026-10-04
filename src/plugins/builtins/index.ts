/**
 * 内置插件:随应用提供的基础功能,同时作为插件 API 的示例
 */

import { playCountPlugin } from './playCount'
import type { BuiltinPluginDefinition } from '../pluginManager'

const builtinPlugins: BuiltinPluginDefinition[] = [playCountPlugin]

export default builtinPlugins
