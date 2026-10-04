use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

mod connections;
pub use connections::{
    Anchor, Connection, MAX_CONNECTIONS, MAX_DOCUMENT_BYTES, MAX_DOCUMENT_OBJECTS,
    MAX_DOCUMENT_POINTS, MAX_JSON_BYTES, MAX_OPERATIONS,
};

pub const MAX_PAGES: usize = 500;
pub const MIN_BRUSH_WIDTH: f32 = 0.1;
pub const MAX_BRUSH_WIDTH: f32 = 100.0;
pub const FILE_VERSION: u32 = 2;
pub const MAX_MATH_DEPTH: usize = 32;
pub const MAX_MATH_NODES: usize = 512;
pub const MAX_MATH_TEXT_BYTES: usize = 4096;
pub const MAX_HISTORY_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_HISTORY_ENTRIES: usize = 100;

pub fn new_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{:x}-{:x}-{:x}",
        std::process::id(),
        time,
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Default for Color {
    fn default() -> Self {
        Self {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct StrokePoint {
    pub x: f32,
    pub y: f32,
    pub time: f64,
    pub pressure: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Style {
    pub color: Color,
    pub width: f32,
    pub dashed: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            color: Color::default(),
            width: 3.0,
            dashed: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShapeKind {
    Line,
    Rectangle,
    Square,
    Triangle,
    RightTriangle,
    EquilateralTriangle,
    Parallelogram,
    Rhombus,
    Ellipse,
    Circle,
    Cube,
    Cuboid,
    Cylinder,
    Cone,
    Sphere,
}

/// Independent, persisted mathematical layout; text leaves are never parsed as formulas.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MathLayout {
    Text(String),
    Row(Vec<MathLayout>),
    Fraction(Box<MathLayout>, Box<MathLayout>),
    Radical(Box<MathLayout>),
}

impl MathLayout {
    pub fn validate(&self) -> Result<()> {
        self.measure().map(|_| ())
    }

    fn measure(&self) -> Result<(usize, usize)> {
        let mut stack = vec![(self, 1usize)];
        let mut nodes = 0usize;
        let mut bytes = 0usize;
        while let Some((layout, depth)) = stack.pop() {
            nodes += 1;
            if depth > MAX_MATH_DEPTH || nodes > MAX_MATH_NODES {
                return Err(Error::InvalidDocument("数学布局深度或节点数超限".into()));
            }
            match layout {
                Self::Text(text) => {
                    bytes = bytes.saturating_add(text.len());
                    if bytes > MAX_MATH_TEXT_BYTES {
                        return Err(Error::InvalidDocument("数学布局文字超过 4096 字节".into()));
                    }
                }
                Self::Row(children) => {
                    if children.len() > MAX_MATH_NODES.saturating_sub(nodes + stack.len()) {
                        return Err(Error::InvalidDocument("数学布局节点数超限".into()));
                    }
                    stack.extend(children.iter().map(|child| (child, depth + 1)));
                }
                Self::Fraction(numerator, denominator) => {
                    stack.push((numerator, depth + 1));
                    stack.push((denominator, depth + 1));
                }
                Self::Radical(child) => stack.push((child, depth + 1)),
            }
        }
        Ok((nodes, bytes))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BoardObject {
    pub id: String,
    pub kind: ObjectKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ObjectKind {
    Stroke {
        points: Vec<StrokePoint>,
        style: Style,
    },
    Shape {
        shape: ShapeKind,
        points: Vec<Point>,
        style: Style,
    },
    Text {
        position: Point,
        text: String,
        size: f32,
        color: Color,
    },
    Math {
        position: Point,
        layout: MathLayout,
        size: f32,
        color: Color,
    },
    Image {
        position: Point,
        width: f32,
        height: f32,
        asset_ref: String,
    },
    CoordinateSystem {
        origin: Point,
        scale: f32,
    },
    FunctionPlot {
        position: Point,
        width: f32,
        height: f32,
        expressions: Vec<String>,
        x_min: f64,
        x_max: f64,
        y_min: f64,
        y_max: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Page {
    pub id: String,
    pub objects: Vec<BoardObject>,
}

impl Page {
    pub fn new() -> Self {
        Self {
            id: new_id(),
            objects: Vec::new(),
        }
    }
}

impl Default for Page {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(try_from = "DocumentData")]
pub struct Document {
    pub id: String,
    pub pages: Vec<Page>,
    pub current_page: usize,
    pub revision: u64,
    #[serde(default)]
    pub connections: Vec<Connection>,
}

#[derive(Deserialize)]
struct DocumentData {
    id: String,
    pages: Vec<Page>,
    current_page: usize,
    revision: u64,
    #[serde(default)]
    connections: Vec<Connection>,
}

impl TryFrom<DocumentData> for Document {
    type Error = Error;
    fn try_from(data: DocumentData) -> Result<Self> {
        let document = Self {
            id: data.id,
            pages: data.pages,
            current_page: data.current_page,
            revision: data.revision,
            connections: data.connections,
        };
        document.validate()?;
        Ok(document)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Operation {
    Add { object: BoardObject },
    Update { object: BoardObject },
    Delete { id: String },
}

#[derive(Debug)]
pub enum Error {
    InvalidDocument(String),
    InvalidObject { id: String, reason: String },
    DuplicateId(String),
    PageNotFound(String),
    ObjectNotFound(String),
    RevisionConflict { expected: u64, actual: u64 },
    RevisionOverflow,
    PageLimit,
    LastPage,
    HistoryDiverged,
    UnsupportedVersion(u64),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDocument(reason) => write!(f, "无效文档：{reason}"),
            Self::InvalidObject { id, reason } => write!(f, "无效对象 {id}：{reason}"),
            Self::DuplicateId(id) => write!(f, "重复 ID：{id}"),
            Self::PageNotFound(id) => write!(f, "页面不存在：{id}"),
            Self::ObjectNotFound(id) => write!(f, "对象不存在：{id}"),
            Self::RevisionConflict { expected, actual } => {
                write!(f, "版本冲突：请求 {expected}，当前 {actual}")
            }
            Self::RevisionOverflow => write!(f, "文档版本已达上限"),
            Self::PageLimit => write!(f, "最多允许 {MAX_PAGES} 页"),
            Self::LastPage => write!(f, "不能删除最后一页"),
            Self::HistoryDiverged => write!(f, "文档已在历史记录之外修改"),
            Self::UnsupportedVersion(version) => write!(f, "不支持文件版本 {version}"),
            Self::Io(error) => error.fmt(f),
            Self::Json(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

fn finite_point(p: &Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}
fn positive(value: f32) -> bool {
    value.is_finite() && value > 0.0
}
fn valid_style(style: &Style) -> bool {
    (MIN_BRUSH_WIDTH..=MAX_BRUSH_WIDTH).contains(&style.width)
}

impl BoardObject {
    pub fn validate(&self) -> Result<()> {
        let mut budget = connections::Budget::default();
        budget.object(self)?;
        let valid = !self.id.trim().is_empty()
            && match &self.kind {
                ObjectKind::Stroke { points, style } => {
                    !points.is_empty()
                        && valid_style(style)
                        && points.iter().all(|p| {
                            p.x.is_finite()
                                && p.y.is_finite()
                                && p.time.is_finite()
                                && p.time >= 0.0
                                && (0.0..=1.0).contains(&p.pressure)
                        })
                }
                ObjectKind::Shape { points, style, .. } => {
                    points.len() >= 2 && points.iter().all(finite_point) && valid_style(style)
                }
                ObjectKind::Text { position, size, .. }
                | ObjectKind::Math { position, size, .. } => {
                    finite_point(position) && positive(*size)
                }
                ObjectKind::Image {
                    position,
                    width,
                    height,
                    asset_ref,
                } => {
                    finite_point(position)
                        && positive(*width)
                        && positive(*height)
                        && !asset_ref.trim().is_empty()
                }
                ObjectKind::CoordinateSystem { origin, scale } => {
                    finite_point(origin) && positive(*scale)
                }
                ObjectKind::FunctionPlot {
                    position,
                    width,
                    height,
                    expressions,
                    x_min,
                    x_max,
                    y_min,
                    y_max,
                } => {
                    finite_point(position)
                        && positive(*width)
                        && positive(*height)
                        && !expressions.is_empty()
                        && expressions.iter().all(|e| !e.trim().is_empty())
                        && [x_min, x_max, y_min, y_max].iter().all(|v| v.is_finite())
                        && x_min < x_max
                        && y_min < y_max
                }
            };
        if valid {
            Ok(())
        } else {
            Err(Error::InvalidObject {
                id: self.id.clone(),
                reason: "ID、坐标、点数或绘制参数超出有效范围".into(),
            })
        }
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        Self {
            id: new_id(),
            pages: vec![Page::new()],
            current_page: 0,
            revision: 0,
            connections: Vec::new(),
        }
    }
    pub fn current_page(&self) -> &Page {
        &self.pages[self.current_page]
    }
    pub fn validate(&self) -> Result<()> {
        self.validate_content()?;
        self.validate_connections()
    }
    fn validate_content(&self) -> Result<()> {
        if self.id.trim().is_empty()
            || self.pages.is_empty()
            || self.current_page >= self.pages.len()
        {
            return Err(Error::InvalidDocument(
                "ID 为空、没有页面或当前页越界".into(),
            ));
        }
        if self.pages.len() > MAX_PAGES {
            return Err(Error::PageLimit);
        }
        connections::check_budget(self)?;
        let mut pages = HashSet::new();
        let mut objects = HashSet::new();
        for page in &self.pages {
            if page.id.trim().is_empty() {
                return Err(Error::InvalidDocument("页面 ID 为空".into()));
            }
            if !pages.insert(&page.id) {
                return Err(Error::DuplicateId(page.id.clone()));
            }
            for object in &page.objects {
                object.validate()?;
                if !objects.insert(&object.id) {
                    return Err(Error::DuplicateId(object.id.clone()));
                }
            }
        }
        Ok(())
    }
    fn next_revision(&self) -> Result<u64> {
        self.revision.checked_add(1).ok_or(Error::RevisionOverflow)
    }
    fn check_revision(&self, expected_revision: u64) -> Result<()> {
        if self.revision != expected_revision {
            return Err(Error::RevisionConflict {
                expected: expected_revision,
                actual: self.revision,
            });
        }
        Ok(())
    }
    pub fn apply(
        &mut self,
        page_id: &str,
        expected_revision: u64,
        operations: &[Operation],
    ) -> Result<()> {
        // #region debug-point B:document-apply-stages
        #[cfg(test)]
        let mut probe = std::time::Instant::now();
        // #endregion
        self.check_revision(expected_revision)?;
        self.validate()?;
        #[cfg(test)]
        tests::history_probe(&mut probe, "document_input_validate");
        let index = self
            .pages
            .iter()
            .position(|p| p.id == page_id)
            .ok_or_else(|| Error::PageNotFound(page_id.into()))?;
        if operations.len() > MAX_OPERATIONS {
            return Err(Error::InvalidDocument("单次编辑操作超过 10000".into()));
        }
        let mut budget = connections::Budget::default();
        for operation in operations {
            match operation {
                Operation::Add { object } | Operation::Update { object } => {
                    budget.object(object)?;
                    object.validate()?;
                }
                Operation::Delete { id } => budget.bytes(id.len())?,
            }
        }
        #[cfg(test)]
        tests::history_probe(&mut probe, "operations_validate");
        // Indexed slots preserve drawing order without quadratic lookup/removal per operation.
        let mut candidate = self.clone();
        #[cfg(test)]
        tests::history_probe(&mut probe, "document_candidate_clone");
        let mut ids: HashSet<String> = self
            .pages
            .iter()
            .flat_map(|p| p.objects.iter().map(|o| o.id.clone()))
            .collect();
        let mut slots: Vec<_> = std::mem::take(&mut candidate.pages[index].objects)
            .into_iter()
            .map(Some)
            .collect();
        let mut positions: HashMap<String, usize> = slots
            .iter()
            .enumerate()
            .map(|(i, o)| (o.as_ref().unwrap().id.clone(), i))
            .collect();
        let mut deleted = HashSet::new();
        for operation in operations {
            match operation {
                Operation::Add { object } => {
                    if !ids.insert(object.id.clone()) {
                        return Err(Error::DuplicateId(object.id.clone()));
                    }
                    positions.insert(object.id.clone(), slots.len());
                    slots.push(Some(object.clone()));
                }
                Operation::Update { object } => {
                    let slot = positions
                        .get(&object.id)
                        .ok_or_else(|| Error::ObjectNotFound(object.id.clone()))?;
                    slots[*slot] = Some(object.clone());
                }
                Operation::Delete { id } => {
                    let slot = positions
                        .remove(id)
                        .ok_or_else(|| Error::ObjectNotFound(id.clone()))?;
                    slots[slot] = None;
                    ids.remove(id);
                    deleted.insert(id.as_str());
                }
            }
        }
        candidate.pages[index].objects = slots.into_iter().flatten().collect();
        candidate.connections.retain(|c| {
            !deleted.contains(c.line_id.as_str()) && !deleted.contains(c.target_id.as_str())
        });
        #[cfg(test)]
        tests::history_probe(&mut probe, "operations_and_indexes");
        candidate.propagate_connections()?;
        #[cfg(test)]
        tests::history_probe(&mut probe, "propagate_and_content_validate");
        if !same_content(&candidate, self) {
            candidate.revision = self.next_revision()?;
            *self = candidate;
        }
        Ok(())
    }
    pub fn add_page(&mut self) -> Result<String> {
        self.validate()?;
        if self.pages.len() == MAX_PAGES {
            return Err(Error::PageLimit);
        }
        let revision = self.next_revision()?;
        let page = Page::new();
        let id = page.id.clone();
        let mut candidate = self.clone();
        candidate.pages.push(page);
        connections::check_budget(&candidate)?;
        candidate.current_page = candidate.pages.len() - 1;
        candidate.revision = revision;
        *self = candidate;
        Ok(id)
    }
    pub fn delete_page(&mut self, page_id: &str) -> Result<()> {
        self.validate()?;
        let index = self
            .pages
            .iter()
            .position(|p| p.id == page_id)
            .ok_or_else(|| Error::PageNotFound(page_id.into()))?;
        if self.pages.len() == 1 {
            return Err(Error::LastPage);
        }
        let revision = self.next_revision()?;
        self.pages.remove(index);
        self.connections.retain(|c| c.page_id != page_id);
        if index < self.current_page {
            self.current_page -= 1;
        }
        self.current_page = self.current_page.min(self.pages.len() - 1);
        self.revision = revision;
        Ok(())
    }
    // 翻页属于视图操作，不产生内容版本，也不使文档变脏。
    pub fn set_current_page(&mut self, index: usize) -> Result<()> {
        if index >= self.pages.len() {
            return Err(Error::PageNotFound(index.to_string()));
        }
        self.current_page = index;
        Ok(())
    }
    /// Smallest persisted schema capable of representing this document.
    pub fn file_version(&self) -> u32 {
        if self
            .pages
            .iter()
            .flat_map(|page| &page.objects)
            .any(|object| matches!(object.kind, ObjectKind::Math { .. }))
        {
            FILE_VERSION
        } else {
            1
        }
    }
    pub fn to_json(&self) -> Result<String> {
        to_json(self)
    }
    pub fn from_json(json: &str) -> Result<Self> {
        from_json(json)
    }
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        save(self, path)
    }
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        load(path)
    }
}

// 比较内容而非版本，撤销回到保存点后仍应恢复干净状态。
fn same_content(a: &Document, b: &Document) -> bool {
    a.id == b.id && a.pages == b.pages && a.connections == b.connections
}

#[derive(Debug, Clone)]
pub struct History {
    undo: Vec<(Document, usize)>,
    redo: Vec<(Document, usize)>,
    saved: Document,
    tracked: Document,
}

fn push_snapshot(stack: &mut Vec<(Document, usize)>, document: &Document) {
    let bytes = connections::document_bytes(document);
    let mut total: usize = stack.iter().map(|(_, size)| size).sum();
    while !stack.is_empty()
        && (stack.len() >= MAX_HISTORY_ENTRIES || total + bytes > MAX_HISTORY_BYTES)
    {
        total -= stack.remove(0).1;
    }
    stack.push((document.clone(), bytes));
}

impl History {
    pub fn new(document: &Document) -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            saved: document.clone(),
            tracked: document.clone(),
        }
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn is_dirty(&self, document: &Document) -> bool {
        !same_content(&self.saved, document)
    }
    pub fn mark_saved(&mut self, document: &Document) {
        self.saved = document.clone();
    }
    fn check_tracked(&self, document: &Document) -> Result<()> {
        document.validate()?;
        if !same_content(&self.tracked, document) || self.tracked.revision != document.revision {
            return Err(Error::HistoryDiverged);
        }
        Ok(())
    }
    // 页面操作与对象操作共用事务入口，整个闭包最多产生一条历史记录。
    pub fn edit<T>(
        &mut self,
        document: &mut Document,
        edit: impl FnOnce(&mut Document) -> Result<T>,
    ) -> Result<T> {
        // #region debug-point B:history-edit-stages
        #[cfg(test)]
        let mut probe = std::time::Instant::now();
        // #endregion
        self.check_tracked(document)?;
        #[cfg(test)]
        tests::history_probe(&mut probe, "history_tracked_validate_compare");
        let mut candidate = document.clone();
        #[cfg(test)]
        tests::history_probe(&mut probe, "history_candidate_clone");
        let result = edit(&mut candidate)?;
        #[cfg(test)]
        tests::history_probe(&mut probe, "history_edit_body_inclusive");
        candidate.validate()?;
        #[cfg(test)]
        tests::history_probe(&mut probe, "history_final_validate");
        if candidate.id != document.id {
            return Err(Error::InvalidDocument("不能在编辑中更换文档 ID".into()));
        }
        if !same_content(document, &candidate) {
            candidate.revision = document.next_revision()?;
            #[cfg(test)]
            tests::history_probe(&mut probe, "history_content_compare");
            push_snapshot(&mut self.undo, document);
            #[cfg(test)]
            tests::history_probe(&mut probe, "history_snapshot_budget_clone");
            self.redo.clear();
        } else {
            candidate.revision = document.revision;
        }
        self.tracked = candidate.clone();
        *document = candidate;
        #[cfg(test)]
        tests::history_probe(&mut probe, "history_tracked_clone_commit");
        Ok(result)
    }
    pub fn apply(
        &mut self,
        document: &mut Document,
        page_id: &str,
        expected_revision: u64,
        operations: &[Operation],
    ) -> Result<()> {
        self.edit(document, |d| {
            d.apply(page_id, expected_revision, operations)
        })
    }
    pub fn undo(&mut self, document: &mut Document) -> Result<bool> {
        self.check_tracked(document)?;
        let Some((previous, _)) = self.undo.last() else {
            return Ok(false);
        };
        let mut candidate = previous.clone();
        candidate.revision = document.next_revision()?;
        push_snapshot(&mut self.redo, document);
        self.undo.pop();
        self.tracked = candidate.clone();
        *document = candidate;
        Ok(true)
    }
    pub fn redo(&mut self, document: &mut Document) -> Result<bool> {
        self.check_tracked(document)?;
        let Some((next, _)) = self.redo.last() else {
            return Ok(false);
        };
        let mut candidate = next.clone();
        candidate.revision = document.next_revision()?;
        push_snapshot(&mut self.undo, document);
        self.redo.pop();
        self.tracked = candidate.clone();
        *document = candidate;
        Ok(true)
    }
    pub fn save(&mut self, document: &Document, path: impl AsRef<Path>) -> Result<()> {
        save(document, path)?;
        self.mark_saved(document);
        Ok(())
    }
}

#[derive(Deserialize)]
struct FileDocument {
    document: Document,
}

#[derive(Serialize)]
struct FileDocumentRef<'a> {
    version: u64,
    document: &'a Document,
}

pub fn to_json(document: &Document) -> Result<String> {
    document.validate()?;
    let json = serde_json::to_string_pretty(&FileDocumentRef {
        version: document.file_version().into(),
        document,
    })?;
    if json.len() > MAX_JSON_BYTES {
        return Err(Error::InvalidDocument("JSON 文件超过 128 MiB".into()));
    }
    Ok(json)
}
pub fn from_json(json: &str) -> Result<Document> {
    if json.len() > MAX_JSON_BYTES {
        return Err(Error::InvalidDocument("JSON 文件超过 128 MiB".into()));
    }
    // Ignore document values on the version pass instead of allocating a JSON Value tree.
    #[derive(Deserialize)]
    struct FileVersion {
        version: Option<u64>,
    }
    let version: FileVersion = serde_json::from_str(json)?;
    let version = version
        .version
        .ok_or_else(|| Error::InvalidDocument("缺少有效文件版本".into()))?;
    if !(1..=u64::from(FILE_VERSION)).contains(&version) {
        return Err(Error::UnsupportedVersion(version));
    }
    let file: FileDocument = serde_json::from_str(json)?;
    if u64::from(file.document.file_version()) > version {
        return Err(Error::InvalidDocument("数学对象需要文件版本 2".into()));
    }
    Ok(file.document)
}
pub fn load(path: impl AsRef<Path>) -> Result<Document> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > MAX_JSON_BYTES as u64 {
        return Err(Error::InvalidDocument("JSON 文件超过 128 MiB".into()));
    }
    let mut json = String::new();
    file.take(MAX_JSON_BYTES as u64 + 1)
        .read_to_string(&mut json)?;
    from_json(&json)
}

pub fn save(document: &Document, path: impl AsRef<Path>) -> Result<()> {
    let json = to_json(document)?;
    let path = path.as_ref();
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if path.file_name().is_none() {
        return Err(Error::InvalidDocument("保存路径缺少文件名".into()));
    }
    let temporary = parent.join(format!(".board-{}.tmp", new_id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| -> Result<()> {
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        // Windows 也必须先关闭临时文件；rename 原子替换且不提前删除目标。
        drop(file);
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod connection_tests;
#[cfg(test)]
mod tests;
