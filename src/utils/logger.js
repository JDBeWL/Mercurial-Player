/**
 * 日志系统
 * 
 * 提供统一的日志管理，支持不同日志级别和环境配置
 * 在生产环境自动禁用调试日志
 */

// 日志级别枚举
export const LogLevel = {
  DEBUG: 0,
  INFO: 1,
  WARN: 2,
  ERROR: 3,
  NONE: 4
};

// 日志级别名称映射
const LEVEL_NAMES = {
  [LogLevel.DEBUG]: 'DEBUG',
  [LogLevel.INFO]: 'INFO',
  [LogLevel.WARN]: 'WARN',
  [LogLevel.ERROR]: 'ERROR'
};

// 日志级别颜色映射（用于控制台输出）
const LEVEL_COLORS = {
  [LogLevel.DEBUG]: '#888',
  [LogLevel.INFO]: '#2196F3',
  [LogLevel.WARN]: '#FF9800',
  [LogLevel.ERROR]: '#F44336'
};

/**
 * 日志系统类
 */
class Logger {
  constructor() {
    // 获取环境变量
    this.isDev = import.meta.env.DEV;
    this.isDebug = import.meta.env.MODE === 'development' || import.meta.env.DEBUG === 'true';
    
    // 根据环境设置默认日志级别
    this.minLevel = this.isDev || this.isDebug ? LogLevel.DEBUG : LogLevel.INFO;
    
    // 是否启用控制台输出
    this.enableConsole = true;
    
    // 是否启用文件输出（通过Tauri后端）
    this.enableFile = false;
    
    // 日志历史（用于调试）
    this.logHistory = [];
    this.maxHistorySize = 100;
  }

  /**
   * 设置最小日志级别
   * @param {number} level - 日志级别
   */
  setMinLevel(level) {
    this.minLevel = level;
  }

  /**
   * 启用/禁用控制台输出
   * @param {boolean} enable - 是否启用
   */
  setConsoleEnabled(enable) {
    this.enableConsole = enable;
  }

  /**
   * 启用/禁用文件输出
   * @param {boolean} enable - 是否启用
   */
  setFileEnabled(enable) {
    this.enableFile = enable;
  }

  /**
   * 格式化时间戳
   * @returns {string} 格式化的时间字符串
   */
  formatTimestamp() {
    const now = new Date();
    const hours = String(now.getHours()).padStart(2, '0');
    const minutes = String(now.getMinutes()).padStart(2, '0');
    const seconds = String(now.getSeconds()).padStart(2, '0');
    const milliseconds = String(now.getMilliseconds()).padStart(3, '0');
    return `${hours}:${minutes}:${seconds}.${milliseconds}`;
  }

  /**
   * 格式化日志消息
   * @param {number} level - 日志级别
   * @param {string} message - 日志消息
   * @param {Array} args - 额外参数
   * @returns {Object} 格式化后的日志对象
   */
  formatLog(level, message, args = []) {
    const timestamp = this.formatTimestamp();
    const levelName = LEVEL_NAMES[level];
    
    return {
      timestamp,
      level: levelName,
      levelValue: level,
      message,
      args: args.length > 0 ? args : undefined,
      stack: level >= LogLevel.ERROR && args[0] instanceof Error 
        ? args[0].stack 
        : undefined
    };
  }

  /**
   * 输出日志到控制台
   * @param {Object} logData - 格式化的日志数据
   */
  outputToConsole(logData) {
    if (!this.enableConsole) return;

    const { timestamp, level, levelValue, message, args } = logData;
    const levelName = LEVEL_NAMES[levelValue];
    const color = LEVEL_COLORS[levelValue];
    
    // 构建控制台输出样式
    const style = `color: ${color}; font-weight: bold;`;
    const prefix = `%c[${timestamp}] [${levelName}]`;
    
    // 根据日志级别选择不同的控制台方法
    const consoleMethod = levelValue === LogLevel.ERROR ? console.error
      : levelValue === LogLevel.WARN ? console.warn
      : levelValue === LogLevel.DEBUG ? console.debug
      : console.log;

    // 输出日志
    if (args && args.length > 0) {
      consoleMethod(prefix, style, message, ...args);
    } else {
      consoleMethod(prefix, style, message);
    }
  }

  /**
   * 输出日志到文件（通过Tauri后端）
   * @param {Object} logData - 格式化的日志数据
   */
  async outputToFile(logData) {
    if (!this.enableFile) return;

    try {
      // 动态导入以避免在非Tauri环境中出错
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('write_log', { logData });
    } catch (error) {
      // 如果后端不支持日志写入，静默失败
      // 避免在日志系统中产生循环错误
    }
  }

  /**
   * 记录日志历史
   * @param {Object} logData - 格式化的日志数据
   */
  recordHistory(logData) {
    this.logHistory.push(logData);
    
    // 限制历史记录大小
    if (this.logHistory.length > this.maxHistorySize) {
      this.logHistory.shift();
    }
  }

  /**
   * 核心日志方法
   * @param {number} level - 日志级别
   * @param {string} message - 日志消息
   * @param {...any} args - 额外参数
   */
  log(level, message, ...args) {
    // 检查日志级别
    if (level < this.minLevel) {
      return;
    }

    // 格式化日志
    const logData = this.formatLog(level, message, args);

    // 记录到历史
    this.recordHistory(logData);

    // 输出到控制台
    this.outputToConsole(logData);

    // 输出到文件（异步，不阻塞）
    this.outputToFile(logData).catch(() => {
      // 静默处理文件输出错误
    });
  }

  /**
   * 调试日志
   * @param {string} message - 日志消息
   * @param {...any} args - 额外参数
   */
  debug(message, ...args) {
    this.log(LogLevel.DEBUG, message, ...args);
  }

  /**
   * 信息日志
   * @param {string} message - 日志消息
   * @param {...any} args - 额外参数
   */
  info(message, ...args) {
    this.log(LogLevel.INFO, message, ...args);
  }

  /**
   * 警告日志
   * @param {string} message - 日志消息
   * @param {...any} args - 额外参数
   */
  warn(message, ...args) {
    this.log(LogLevel.WARN, message, ...args);
  }

  /**
   * 错误日志
   * @param {string} message - 日志消息
   * @param {...any} args - 额外参数
   */
  error(message, ...args) {
    this.log(LogLevel.ERROR, message, ...args);
  }

  /**
   * 获取日志历史
   * @param {number} limit - 限制返回的日志数量
   * @returns {Array} 日志历史数组
   */
  getHistory(limit = null) {
    if (limit === null) {
      return [...this.logHistory];
    }
    return this.logHistory.slice(-limit);
  }

  /**
   * 清空日志历史
   */
  clearHistory() {
    this.logHistory = [];
  }

  /**
   * 导出日志历史为文本
   * @returns {string} 日志文本
   */
  exportHistoryAsText() {
    return this.logHistory
      .map(log => {
        const { timestamp, level, message, args, stack } = log;
        let text = `[${timestamp}] [${level}] ${message}`;
        
        if (args && args.length > 0) {
          text += ' ' + args.map(arg => {
            if (arg instanceof Error) {
              return arg.toString();
            }
            try {
              return JSON.stringify(arg);
            } catch {
              return String(arg);
            }
          }).join(' ');
        }
        
        if (stack) {
          text += '\n' + stack;
        }
        
        return text;
      })
      .join('\n');
  }
}

// 创建全局日志实例
const logger = new Logger();

// 导出日志实例和日志级别
export default logger;
export { Logger };

