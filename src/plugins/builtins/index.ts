/**
 * 内置插件
 * 这些插件作为示例和基础功能提供
 */

import { playCountPlugin } from './playCount'
import type { BuiltinPluginDefinition } from '../pluginManager'

const builtinPlugins: BuiltinPluginDefinition[] = [
  playCountPlugin,
]

export default builtinPlugins
