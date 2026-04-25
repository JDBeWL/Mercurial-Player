//! Tantivy 搜索引擎索引模块
//!
//! 提供基于 Tantivy 的全文搜索索引功能，用于快速搜索歌曲、艺术家、专辑等。

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TextFieldIndexing, TextOptions, Value, FAST, STORED,
};
use tantivy::{Index, IndexReader, IndexWriter, TantivyDocument, Term};

use super::metadata::TrackMetadata;

// ============================================================================
// 索引配置
// ============================================================================

/// 索引目录名
const INDEX_DIR_NAME: &str = "tantivy-index";

// ============================================================================
// 全局索引实例
// ============================================================================

/// 全局索引管理器
static INDEX_MANAGER: Mutex<Option<Arc<Mutex<TantivyIndexManager>>>> = Mutex::new(None);

/// 获取或创建索引管理器
fn get_index_manager() -> Result<Arc<Mutex<TantivyIndexManager>>, String> {
    if let Ok(lock) = INDEX_MANAGER.lock() {
        if let Some(manager) = lock.as_ref() {
            return Ok(manager.clone());
        }
    }

    let manager = Arc::new(Mutex::new(TantivyIndexManager::new()?));
    if let Ok(mut lock) = INDEX_MANAGER.lock() {
        *lock = Some(manager.clone());
    }
    Ok(manager)
}

// ============================================================================
// Tantivy 索引管理器
// ============================================================================

/// Tantivy 索引管理器
pub struct TantivyIndexManager {
    /// Tantivy 索引实例
    index: Index,
    /// 索引读取器
    reader: IndexReader,
    /// 索引字段定义
    fields: IndexFields,
    /// 写入器（懒加载）
    writer: Option<IndexWriter>,
}

/// 索引字段定义
struct IndexFields {
    path: Field,
    title: Field,
    artist: Field,
    album: Field,
    name: Field,
    format: Field,
}

impl TantivyIndexManager {
    /// 创建新的索引管理器
    fn new() -> Result<Self, String> {
        let index_dir = index_dir_path();
        fs::create_dir_all(&index_dir).map_err(|e| format!("创建索引目录失败: {e}"))?;

        let (schema, fields) = Self::build_schema();

        let index = if index_dir.join("meta.json").exists() {
            Index::open_in_dir(&index_dir).map_err(|e| format!("打开索引失败: {e}"))?
        } else {
            Index::create_in_dir(&index_dir, schema)
                .map_err(|e| format!("创建索引失败: {e}"))?
        };

        let reader = index
            .reader_builder()
            .try_into()
            .map_err(|e| format!("创建索引读取器失败: {e}"))?;

        Ok(Self {
            index,
            reader,
            fields,
            writer: None,
        })
    }

    /// 构建索引 Schema
    fn build_schema() -> (Schema, IndexFields) {
        let mut schema_builder = Schema::builder();

        // 文件路径 - 存储但不分词，用于唯一标识
        let path = schema_builder.add_text_field("path", STORED);

        // 歌曲标题 - 分词索引，支持全文搜索
        let title = schema_builder.add_text_field(
            "title",
            TextOptions::default()
                .set_indexing_options(
                    TextFieldIndexing::default()
                        .set_tokenizer("default")
                        .set_index_option(IndexRecordOption::WithFreqsAndPositions),
                )
                .set_stored(),
        );

        // 艺术家 - 分词索引
        let artist = schema_builder.add_text_field(
            "artist",
            TextOptions::default()
                .set_indexing_options(
                    TextFieldIndexing::default()
                        .set_tokenizer("default")
                        .set_index_option(IndexRecordOption::WithFreqsAndPositions),
                )
                .set_stored(),
        );

        // 专辑 - 分词索引
        let album = schema_builder.add_text_field(
            "album",
            TextOptions::default()
                .set_indexing_options(
                    TextFieldIndexing::default()
                        .set_tokenizer("default")
                        .set_index_option(IndexRecordOption::WithFreqsAndPositions),
                )
                .set_stored(),
        );

        // 文件名 - 分词索引
        let name = schema_builder.add_text_field(
            "name",
            TextOptions::default()
                .set_indexing_options(
                    TextFieldIndexing::default()
                        .set_tokenizer("default")
                        .set_index_option(IndexRecordOption::WithFreqsAndPositions),
                )
                .set_stored(),
        );

        // 格式 - 存储，用于过滤
        let format = schema_builder.add_text_field("format", STORED | FAST);

        let schema = schema_builder.build();
        let fields = IndexFields {
            path,
            title,
            artist,
            album,
            name,
            format,
        };

        (schema, fields)
    }

    /// 获取或创建写入器
    fn get_writer(&mut self) -> Result<&mut IndexWriter, String> {
        if self.writer.is_none() {
            let writer = self
                .index
                .writer(50_000_000)
                .map_err(|e| format!("创建索引写入器失败: {e}"))?;
            self.writer = Some(writer);
        }

        self.writer
            .as_mut()
            .ok_or_else(|| "写入器未初始化".to_string())
    }

    /// 添加或更新文档
    fn add_document(&mut self, metadata: &TrackMetadata) -> Result<(), String> {
        let mut doc = TantivyDocument::default();

        doc.add_text(self.fields.path, &metadata.path);
        doc.add_text(
            self.fields.title,
            metadata.title.as_deref().unwrap_or(&metadata.name),
        );
        doc.add_text(
            self.fields.artist,
            metadata.artist.as_deref().unwrap_or("Unknown Artist"),
        );
        doc.add_text(
            self.fields.album,
            metadata.album.as_deref().unwrap_or("Unknown Album"),
        );
        doc.add_text(self.fields.name, &metadata.name);
        doc.add_text(
            self.fields.format,
            metadata.format.as_deref().unwrap_or("UNKNOWN"),
        );

        let writer = self.get_writer()?;
        writer
            .add_document(doc)
            .map_err(|e| format!("添加文档失败: {e}"))?;

        Ok(())
    }

    /// 删除文档（按路径）
    fn delete_document(&mut self, path: &str) -> Result<(), String> {
        let term = Term::from_field_text(self.fields.path, path);
        let writer = self.get_writer()?;
        writer.delete_term(term);
        Ok(())
    }

    /// 提交更改
    fn commit(&mut self) -> Result<(), String> {
        if let Some(ref mut writer) = self.writer {
            writer.commit().map_err(|e| format!("提交索引失败: {e}"))?;
            log::info!("索引已提交");
        }
        Ok(())
    }

    /// 搜索文档
    fn search(&self, query: &str, limit: usize) -> Result<Vec<TrackMetadata>, String> {
        let searcher = self.reader.searcher();

        // 构建多字段查询
        let query_parser = QueryParser::for_index(
            &self.index,
            vec![
                self.fields.title,
                self.fields.artist,
                self.fields.album,
                self.fields.name,
            ],
        );

        let parsed_query = query_parser
            .parse_query(query)
            .map_err(|e| format!("解析查询失败: {e}"))?;

        let top_docs = searcher
            .search(&parsed_query, &TopDocs::with_limit(limit))
            .map_err(|e| format!("搜索失败: {e}"))?;

        let mut results = Vec::new();
        for (_score, doc_address) in top_docs {
            if let Ok(retrieved_doc) = searcher.doc(doc_address) {
                if let Some(metadata) = self.document_to_metadata(&retrieved_doc) {
                    results.push(metadata);
                }
            }
        }

        Ok(results)
    }

    /// 将 Tantivy 文档转换为 TrackMetadata
    fn document_to_metadata(&self, doc: &TantivyDocument) -> Option<TrackMetadata> {
        let get_text = |field: Field| -> Option<String> {
            doc.get_first(field)
                .and_then(|v| v.as_str())
                .map(String::from)
        };

        let path = get_text(self.fields.path)?;
        let title = get_text(self.fields.title);
        let artist = get_text(self.fields.artist);
        let album = get_text(self.fields.album);
        let name = get_text(self.fields.name).unwrap_or_else(|| path.clone());
        let format = get_text(self.fields.format);

        Some(TrackMetadata {
            path,
            name,
            title,
            artist,
            album,
            format,
            ..Default::default()
        })
    }

    /// 获取索引中的文档数量
    fn doc_count(&self) -> usize {
        self.reader.searcher().num_docs() as usize
    }
}

// ============================================================================
// 公共 API
// ============================================================================

/// 获取索引目录路径
fn index_dir_path() -> PathBuf {
    if let Some(custom_path) = super::metadata::get_cover_cache_path_setting() {
        return PathBuf::from(custom_path).join(INDEX_DIR_NAME);
    }
    std::env::temp_dir().join("mercurial-player").join(INDEX_DIR_NAME)
}

/// 添加或更新音轨到索引
pub fn index_track(metadata: &TrackMetadata) -> Result<(), String> {
    let manager = get_index_manager()?;
    let mut manager = manager.lock().map_err(|e| format!("获取索引锁失败: {e}"))?;
    manager.add_document(metadata)
}

/// 批量添加音轨到索引
pub fn index_tracks_batch(metadatas: &[TrackMetadata]) -> Result<(), String> {
    let manager = get_index_manager()?;
    let mut manager = manager.lock().map_err(|e| format!("获取索引锁失败: {e}"))?;
    for metadata in metadatas {
        manager.add_document(metadata)?;
    }
    manager.commit()
}

/// 从索引中删除音轨
pub fn remove_track_from_index(path: &str) -> Result<(), String> {
    let manager = get_index_manager()?;
    let mut manager = manager.lock().map_err(|e| format!("获取索引锁失败: {e}"))?;
    manager.delete_document(path)?;
    manager.commit()
}

/// 搜索音轨
pub fn search_tracks(query: &str, limit: usize) -> Result<Vec<TrackMetadata>, String> {
    let manager = get_index_manager()?;
    let manager = manager.lock().map_err(|e| format!("获取索引锁失败: {e}"))?;
    manager.search(query, limit)
}

/// 获取索引文档数量
pub fn get_index_doc_count() -> Result<usize, String> {
    let manager = get_index_manager()?;
    let manager = manager.lock().map_err(|e| format!("获取索引锁失败: {e}"))?;
    Ok(manager.doc_count())
}

/// 提交索引更改
pub fn commit_index() -> Result<(), String> {
    let manager = get_index_manager()?;
    let mut manager = manager.lock().map_err(|e| format!("获取索引锁失败: {e}"))?;
    manager.commit()
}

/// 重建索引
pub fn rebuild_tantivy_index() -> Result<(), String> {
    // 清除现有索引
    let index_dir = index_dir_path();
    if index_dir.exists() {
        fs::remove_dir_all(&index_dir).map_err(|e| format!("删除旧索引失败: {e}"))?;
    }
    fs::create_dir_all(&index_dir).map_err(|e| format!("创建索引目录失败: {e}"))?;

    // 重置全局索引管理器
    if let Ok(mut lock) = INDEX_MANAGER.lock() {
        *lock = None;
    }

    // 重新初始化
    let _manager = get_index_manager()?;

    log::info!("Tantivy 索引已重建");
    Ok(())
}

/// 清除所有索引数据
pub fn clear_tantivy_index() -> Result<(), String> {
    let index_dir = index_dir_path();
    if index_dir.exists() {
        fs::remove_dir_all(&index_dir).map_err(|e| format!("删除索引目录失败: {e}"))?;
    }

    // 重置全局索引管理器
    if let Ok(mut lock) = INDEX_MANAGER.lock() {
        *lock = None;
    }

    log::info!("Tantivy 索引已清除");
    Ok(())
}
