/**
 * 统一错误处理系统
 * 
 * 提供统一的错误处理机制，包括错误分类、日志记录和用户提示
 */

import logger from './logger';

/**
 * 错误类型枚举
 */
export const ErrorType = {
  // 网络相关错误
  NETWORK: 'NETWORK',
  NETWORK_TIMEOUT: 'NETWORK_TIMEOUT',
  NETWORK_OFFLINE: 'NETWORK_OFFLINE',
  
  // 文件系统相关错误
  FILE_NOT_FOUND: 'FILE_NOT_FOUND',
  FILE_READ_ERROR: 'FILE_READ_ERROR',
  FILE_WRITE_ERROR: 'FILE_WRITE_ERROR',
  FILE_PERMISSION_DENIED: 'FILE_PERMISSION_DENIED',
  
  // 音频相关错误
  AUDIO_DECODE_ERROR: 'AUDIO_DECODE_ERROR',
  AUDIO_PLAYBACK_ERROR: 'AUDIO_PLAYBACK_ERROR',
  AUDIO_DEVICE_ERROR: 'AUDIO_DEVICE_ERROR',
  
  // 配置相关错误
  CONFIG_LOAD_ERROR: 'CONFIG_LOAD_ERROR',
  CONFIG_SAVE_ERROR: 'CONFIG_SAVE_ERROR',
  CONFIG_INVALID: 'CONFIG_INVALID',
  
  // 数据相关错误
  DATA_PARSE_ERROR: 'DATA_PARSE_ERROR',
  DATA_VALIDATION_ERROR: 'DATA_VALIDATION_ERROR',
  
  // 未知错误
  UNKNOWN: 'UNKNOWN'
};

/**
 * 错误严重程度枚举
 */
export const ErrorSeverity = {
  LOW: 'LOW',        // 低严重程度，可以忽略或自动恢复
  MEDIUM: 'MEDIUM',  // 中等严重程度，需要用户注意
  HIGH: 'HIGH',      // 高严重程度，影响功能使用
  CRITICAL: 'CRITICAL' // 严重错误，可能导致应用崩溃
};

/**
 * 应用错误类
 */
export class AppError extends Error {
  constructor(message, type = ErrorType.UNKNOWN, severity = ErrorSeverity.MEDIUM, originalError = null, context = {}) {
    super(message);
    this.name = 'AppError';
    this.type = type;
    this.severity = severity;
    this.originalError = originalError;
    this.context = context;
    this.timestamp = new Date().toISOString();
    
    // 保持堆栈跟踪
    if (Error.captureStackTrace) {
      Error.captureStackTrace(this, AppError);
    }
  }

  /**
   * 转换为可序列化的对象
   */
  toJSON() {
    return {
      name: this.name,
      message: this.message,
      type: this.type,
      severity: this.severity,
      timestamp: this.timestamp,
      context: this.context,
      stack: this.stack,
      originalError: this.originalError instanceof Error
        ? {
            name: this.originalError.name,
            message: this.originalError.message,
            stack: this.originalError.stack
          }
        : this.originalError
    };
  }
}

/**
 * 错误处理器类
 */
class ErrorHandler {
  constructor() {
    // 错误监听器列表
    this.listeners = [];
    
    // 错误统计
    this.errorStats = {
      total: 0,
      byType: {},
      bySeverity: {},
      recent: []
    };
    
    // 最大最近错误记录数
    this.maxRecentErrors = 50;
  }

  /**
   * 注册错误监听器
   * @param {Function} listener - 错误监听器函数
   * @returns {Function} 取消注册的函数
   */
  onError(listener) {
    this.listeners.push(listener);
    return () => {
      const index = this.listeners.indexOf(listener);
      if (index > -1) {
        this.listeners.splice(index, 1);
      }
    };
  }

  /**
   * 处理错误
   * @param {Error|AppError|any} error - 错误对象
   * @param {Object} options - 处理选项
   * @returns {AppError} 处理后的错误对象
   */
  handle(error, options = {}) {
    const {
      type = ErrorType.UNKNOWN,
      severity = ErrorSeverity.MEDIUM,
      context = {},
      silent = false,
      showToUser = true,
      userMessage = null
    } = options;

    // 转换为 AppError
    let appError;
    if (error instanceof AppError) {
      appError = error;
      // 合并上下文
      appError.context = { ...appError.context, ...context };
    } else if (error instanceof Error) {
      appError = new AppError(
        error.message || '未知错误',
        type,
        severity,
        error,
        context
      );
    } else {
      appError = new AppError(
        String(error) || '未知错误',
        type,
        severity,
        error,
        context
      );
    }

    // 更新统计
    this.updateStats(appError);

    // 记录错误
    if (!silent) {
      this.logError(appError);
    }

    // 通知监听器
    this.notifyListeners(appError, { showToUser, userMessage });

    return appError;
  }

  /**
   * 更新错误统计
   * @param {AppError} error - 错误对象
   */
  updateStats(error) {
    this.errorStats.total++;
    
    // 按类型统计
    if (!this.errorStats.byType[error.type]) {
      this.errorStats.byType[error.type] = 0;
    }
    this.errorStats.byType[error.type]++;

    // 按严重程度统计
    if (!this.errorStats.bySeverity[error.severity]) {
      this.errorStats.bySeverity[error.severity] = 0;
    }
    this.errorStats.bySeverity[error.severity]++;

    // 记录最近错误
    this.errorStats.recent.push({
      error: error.toJSON(),
      timestamp: new Date().toISOString()
    });

    // 限制最近错误数量
    if (this.errorStats.recent.length > this.maxRecentErrors) {
      this.errorStats.recent.shift();
    }
  }

  /**
   * 记录错误日志
   * @param {AppError} error - 错误对象
   */
  logError(error) {
    const logMessage = `[${error.type}] ${error.message}`;
    const logContext = {
      severity: error.severity,
      context: error.context,
      originalError: error.originalError
    };

    // 根据严重程度选择日志级别
    switch (error.severity) {
      case ErrorSeverity.CRITICAL:
      case ErrorSeverity.HIGH:
        logger.error(logMessage, logContext, error.originalError);
        break;
      case ErrorSeverity.MEDIUM:
        logger.warn(logMessage, logContext);
        break;
      case ErrorSeverity.LOW:
        logger.debug(logMessage, logContext);
        break;
      default:
        logger.error(logMessage, logContext);
    }
  }

  /**
   * 通知错误监听器
   * @param {AppError} error - 错误对象
   * @param {Object} options - 通知选项
   */
  notifyListeners(error, options = {}) {
    const { showToUser = true, userMessage = null } = options;

    this.listeners.forEach(listener => {
      try {
        listener(error, {
          showToUser,
          userMessage: userMessage || this.getUserFriendlyMessage(error)
        });
      } catch (listenerError) {
        // 避免监听器错误导致循环
        logger.error('Error in error listener:', listenerError);
      }
    });
  }

  /**
   * 获取用户友好的错误消息
   * @param {AppError} error - 错误对象
   * @returns {string} 用户友好的错误消息
   */
  getUserFriendlyMessage(error) {
    // 根据错误类型返回友好的消息
    const messages = {
      [ErrorType.NETWORK]: '网络连接失败，请检查网络设置',
      [ErrorType.NETWORK_TIMEOUT]: '网络请求超时，请稍后重试',
      [ErrorType.NETWORK_OFFLINE]: '网络未连接，请检查网络设置',
      [ErrorType.FILE_NOT_FOUND]: '文件未找到，可能已被移动或删除',
      [ErrorType.FILE_READ_ERROR]: '文件读取失败，请检查文件权限',
      [ErrorType.FILE_WRITE_ERROR]: '文件写入失败，请检查磁盘空间和权限',
      [ErrorType.FILE_PERMISSION_DENIED]: '没有文件访问权限',
      [ErrorType.AUDIO_DECODE_ERROR]: '音频解码失败，文件可能已损坏',
      [ErrorType.AUDIO_PLAYBACK_ERROR]: '音频播放失败，请检查音频设备',
      [ErrorType.AUDIO_DEVICE_ERROR]: '音频设备错误，请检查音频设备连接',
      [ErrorType.CONFIG_LOAD_ERROR]: '配置加载失败，将使用默认配置',
      [ErrorType.CONFIG_SAVE_ERROR]: '配置保存失败，请检查磁盘空间',
      [ErrorType.CONFIG_INVALID]: '配置格式无效，已重置为默认配置',
      [ErrorType.DATA_PARSE_ERROR]: '数据解析失败，数据格式可能不正确',
      [ErrorType.DATA_VALIDATION_ERROR]: '数据验证失败，请检查输入数据',
      [ErrorType.UNKNOWN]: '发生未知错误，请稍后重试'
    };

    return messages[error.type] || error.message || '发生错误，请稍后重试';
  }

  /**
   * 获取错误统计信息
   * @returns {Object} 错误统计信息
   */
  getStats() {
    return {
      ...this.errorStats,
      recent: [...this.errorStats.recent]
    };
  }

  /**
   * 清空错误统计
   */
  clearStats() {
    this.errorStats = {
      total: 0,
      byType: {},
      bySeverity: {},
      recent: []
    };
  }
}

// 创建全局错误处理器实例
const errorHandler = new ErrorHandler();

/**
 * 异步操作错误处理包装器
 * @param {Function} asyncFn - 异步函数
 * @param {Object} options - 错误处理选项
 * @returns {Function} 包装后的函数
 */
export function withErrorHandling(asyncFn, options = {}) {
  return async (...args) => {
    try {
      return await asyncFn(...args);
    } catch (error) {
      const handledError = errorHandler.handle(error, options);
      
      // 如果设置了 throw，则重新抛出错误
      if (options.throw !== false) {
        throw handledError;
      }
      
      // 否则返回错误结果
      return {
        success: false,
        error: handledError,
        data: null
      };
    }
  };
}

/**
 * Promise 错误处理包装器
 * @param {Promise} promise - Promise 对象
 * @param {Object} options - 错误处理选项
 * @returns {Promise} 处理后的 Promise
 */
export async function handlePromise(promise, options = {}) {
  try {
    const result = await promise;
    return {
      success: true,
      data: result,
      error: null
    };
  } catch (error) {
    const handledError = errorHandler.handle(error, options);
    return {
      success: false,
      data: null,
      error: handledError
    };
  }
}

/**
 * 创建错误处理装饰器（用于类方法）
 * @param {Object} options - 错误处理选项
 * @returns {Function} 装饰器函数
 */
export function errorHandlerDecorator(options = {}) {
  return function(target, propertyKey, descriptor) {
    const originalMethod = descriptor.value;

    descriptor.value = async function(...args) {
      try {
        return await originalMethod.apply(this, args);
      } catch (error) {
        const handledError = errorHandler.handle(error, {
          ...options,
          context: {
            ...options.context,
            method: propertyKey,
            className: target.constructor.name
          }
        });
        
        if (options.throw !== false) {
          throw handledError;
        }
        
        return {
          success: false,
          error: handledError,
          data: null
        };
      }
    };

    return descriptor;
  };
}

// 导出错误处理器实例和工具函数
export default errorHandler;
export { ErrorHandler };

