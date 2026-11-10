//! 在电脑端提取常见办公文件的只读内容；不执行宏、公式或外部链接。
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Cursor, Read, Write};
use std::path::Path;

use calamine::{
    DataRef, FontStyle, FontWeight, HorizontalAlignment, Reader, UnderlineStyle, VerticalAlignment,
    Xlsx,
};
use roxmltree::{Document, Node, ParsingOptions};
use serde::Serialize;
use serde_json::{Value, json};
use zip::ZipArchive;

// 用户要求手机在线读表格与文档：输入沿用文件接口 4 MiB 上限；解压最多 32 MiB、
// 单个 XML 16 MiB、4096 个成员。预览最多 20 张表、每表 1000 行/100 列、总计 10 万格，
// 正文总量 1 MiB、单格 16 KiB；与 session-files.js 的响应校验和分页联动。
const MAX_ARCHIVE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_XML_BYTES: usize = 16 * 1024 * 1024;
const MAX_SHEETS: usize = 20;
const MAX_ROWS: usize = 1000;
const MAX_COLUMNS: usize = 100;
const MAX_CELLS: usize = 100_000;
const MAX_TEXT: usize = 1024 * 1024;
const MAX_CELL_TEXT: usize = 16 * 1024;
const MAX_SECTIONS: usize = 100;
const MAX_BLOCKS: usize = 2000;
const MAX_MERGES: usize = 10_000;
const DEFAULT_COLUMN_WIDTH: u16 = 120;
const MAX_COLUMN_WIDTH: u16 = 480;
const DEFAULT_ROW_HEIGHT: u16 = 20;
const MAX_ROW_HEIGHT: u16 = 2048;
type PreviewResult<T> = Result<T, &'static str>;
type OfficeZip<'a> = ZipArchive<Cursor<&'a [u8]>>;

#[derive(Default)]
struct Budget {
    bytes: usize,
    cells: usize,
    style_cells: usize,
    blocks: usize,
    truncated: bool,
}

impl Budget {
    fn text(&mut self, value: &str) -> String {
        let limit = MAX_CELL_TEXT.min(MAX_TEXT.saturating_sub(self.bytes));
        let mut end = value.len().min(limit);
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        self.bytes += end;
        self.truncated |= end < value.len();
        value[..end].to_owned()
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Sheet {
    name: String,
    rows: Vec<Row>,
    column_count: usize,
    merges: Vec<MergeRange>,
    // 无原始列宽（CSV/TSV/ODS）序列化为 []，前端按默认列宽读取；非空时每列必须对应一个宽度。
    column_widths: Vec<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default_row_height: Option<u16>,
    styles: Vec<CellStyle>,
    truncated: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MergeRange {
    start_row: u32,
    end_row: u32,
    start_column: usize,
    end_column: usize,
}

#[derive(Default)]
struct SheetLayout {
    merges: Vec<MergeRange>,
    widths: Vec<u16>,
    default_row_height: Option<u16>,
    row_heights: HashMap<u32, u16>,
}

#[derive(Serialize)]
struct Row {
    number: u32,
    cells: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    height: Option<u16>,
    #[serde(rename = "styleIds", skip_serializing_if = "Vec::is_empty")]
    style_ids: Vec<usize>,
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct CellStyle {
    background: Option<String>,
    color: Option<String>,
    font_size: Option<f64>,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    horizontal: &'static str,
    vertical: &'static str,
    wrap: bool,
}

fn cell_style(style: &calamine::Style) -> CellStyle {
    let font = style.font.as_ref();
    let alignment = style.alignment.as_ref();
    CellStyle {
        background: style
            .fill
            .as_ref()
            .filter(|fill| fill.is_visible())
            .and_then(|fill| fill.get_color())
            .map(|color| color.to_string()),
        color: font
            .and_then(|font| font.color)
            .map(|color| color.to_string()),
        font_size: font
            .and_then(|font| font.size)
            .filter(|size| size.is_finite())
            .map(|size| size.clamp(6.0, 72.0)),
        bold: font.is_some_and(|font| font.weight == FontWeight::Bold),
        italic: font.is_some_and(|font| font.style == FontStyle::Italic),
        underline: font.is_some_and(|font| font.underline != UnderlineStyle::None),
        strike: font.is_some_and(|font| font.strikethrough),
        horizontal: match alignment.map(|value| value.horizontal) {
            Some(HorizontalAlignment::Center | HorizontalAlignment::CenterContinuous) => "center",
            Some(HorizontalAlignment::Right) => "right",
            Some(HorizontalAlignment::Justify | HorizontalAlignment::Distributed) => "justify",
            _ => "left",
        },
        vertical: match alignment.map(|value| value.vertical) {
            Some(VerticalAlignment::Center) => "middle",
            Some(VerticalAlignment::Bottom) => "bottom",
            _ => "top",
        },
        wrap: alignment.is_some_and(|value| value.wrap_text),
    }
}

#[derive(Default)]
struct SheetBuilder {
    rows: BTreeMap<u32, Vec<String>>,
    truncated: bool,
}

impl SheetBuilder {
    fn insert(&mut self, row: u32, column: usize, text: &str, budget: &mut Budget) -> bool {
        if column >= MAX_COLUMNS || self.rows.len() >= MAX_ROWS && !self.rows.contains_key(&row) {
            self.truncated = true;
            return false;
        }
        let cells = self.rows.entry(row).or_default();
        let extra = (column + 1).saturating_sub(cells.len());
        if budget.cells + extra > MAX_CELLS || budget.bytes >= MAX_TEXT {
            self.truncated = true;
            return false;
        }
        budget.cells += extra;
        cells.resize(cells.len().max(column + 1), String::new());
        cells[column] = budget.text(text);
        true
    }

    fn finish(self, name: String) -> Sheet {
        let column_count = self.rows.values().map(Vec::len).max().unwrap_or(0);
        Sheet {
            name,
            rows: self
                .rows
                .into_iter()
                .map(|(number, cells)| Row {
                    number,
                    cells,
                    height: None,
                    style_ids: Vec::new(),
                })
                .collect(),
            column_count,
            merges: Vec::new(),
            column_widths: Vec::new(),
            default_row_height: None,
            styles: Vec::new(),
            truncated: self.truncated,
        }
    }
}

#[derive(Serialize)]
struct Section {
    title: String,
    blocks: Vec<Block>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Block {
    Paragraph { text: String },
    Heading { text: String },
    Table { rows: Vec<Vec<String>> },
}

pub(super) fn preview(path: &str, bytes: &[u8]) -> Option<Value> {
    let extension = Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
    if !matches!(
        extension.as_str(),
        "xlsx"
            | "xlsm"
            | "xltx"
            | "csv"
            | "tsv"
            | "docx"
            | "odt"
            | "ods"
            | "pptx"
            | "pdf"
            | "xls"
            | "doc"
            | "ppt"
    ) {
        return None;
    }
    let result = std::panic::catch_unwind(|| match extension.as_str() {
        "xlsx" | "xlsm" | "xltx" => spreadsheet(bytes),
        "csv" | "tsv" => delimited(bytes, if extension == "tsv" { b'\t' } else { b',' }),
        "docx" | "odt" => document(bytes, &extension),
        "ods" => open_document_sheets(bytes),
        "pptx" => presentation(bytes),
        "pdf" => pdf(bytes),
        _ => Err("旧版 Office 二进制文件请另存为 XLSX、DOCX 或 PPTX 后在线查看"),
    })
    .unwrap_or(Err("文件内容无法解析，可能已损坏或使用了不受支持的格式"));
    Some(result.unwrap_or_else(|message| {
        json!({
            "kind": "unsupported", "truncated": false, "message": message
        })
    }))
}

fn office_zip(bytes: &[u8]) -> PreviewResult<OfficeZip<'_>> {
    let mut zip =
        ZipArchive::new(Cursor::new(bytes)).map_err(|_| "文件不是有效的办公文档，或已加密")?;
    if zip.len() > 4096 {
        return Err("文档内部文件过多，暂不提供在线预览");
    }
    let mut total = 0_u64;
    for index in 0..zip.len() {
        let file = zip
            .by_index(index)
            .map_err(|_| "文档内部文件无法读取，可能已加密")?;
        total = total.saturating_add(file.size());
        if file.encrypted() || file.size() > MAX_XML_BYTES as u64 || total > MAX_ARCHIVE_BYTES {
            return Err("文档已加密或解压后过大，暂不提供在线预览");
        }
    }
    Ok(zip)
}

fn xml_entry(zip: &mut OfficeZip<'_>, path: &str) -> PreviewResult<String> {
    let file = zip
        .by_name(path)
        .map_err(|_| "文档缺少必要内容，可能已损坏")?;
    let mut text = String::new();
    file.take(MAX_XML_BYTES as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|_| "文档内容编码无效或无法解压")?;
    if text.len() > MAX_XML_BYTES {
        return Err("文档内容过大，暂不提供在线预览");
    }
    Ok(text)
}

fn xml(text: &str) -> PreviewResult<Document<'_>> {
    Document::parse_with_options(
        text,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: 300_000,
            ..ParsingOptions::default()
        },
    )
    .map_err(|_| "文档 XML 无效、包含外部实体或内容过于复杂")
}

fn package_part(base: &str, target: &str) -> PreviewResult<String> {
    if target.len() > 16384 || target.contains(['\\', ':', '?', '#']) {
        return Err("Excel 内部内容路径无效");
    }
    let mut parts: Vec<_> = if target.starts_with('/') {
        Vec::new()
    } else {
        base.rsplit_once('/')
            .map(|(parent, _)| parent.split('/').collect())
            .unwrap_or_default()
    };
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop().ok_or("Excel 内部内容路径越界")?;
            }
            part => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

fn cell_position(reference: &str) -> PreviewResult<(u32, usize)> {
    let split = reference
        .find(|value: char| value.is_ascii_digit())
        .ok_or("Excel 合并范围无效")?;
    let mut column = 0_usize;
    for letter in reference[..split].bytes() {
        if !letter.is_ascii_alphabetic() {
            return Err("Excel 合并范围无效");
        }
        column = column
            .checked_mul(26)
            .and_then(|value| value.checked_add((letter.to_ascii_uppercase() - b'A' + 1) as usize))
            .ok_or("Excel 合并范围无效")?;
    }
    let row: u32 = reference[split..]
        .parse()
        .map_err(|_| "Excel 合并范围无效")?;
    if row == 0 || row > 1_048_576 || column == 0 || column > 16_384 {
        return Err("Excel 合并范围无效");
    }
    Ok((row, column - 1))
}

fn spreadsheet_column_width(value: Option<&str>) -> Option<u16> {
    let width = value?
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)?;
    // 用户要求原始布局保留窄列：字符宽按 7px 加 5px 边距近似，限制 1–480px；
    // 紧凑布局的 48px 最小值由前端单独处理，须与 session-files.js 的尺寸校验同步。
    let max_characters = (f64::from(MAX_COLUMN_WIDTH) - 5.0) / 7.0;
    Some(
        (width.min(max_characters) * 7.0 + 5.0)
            .round()
            .clamp(1.0, f64::from(MAX_COLUMN_WIDTH)) as u16,
    )
}

fn spreadsheet_row_height(value: Option<&str>) -> Option<u16> {
    let height = value?
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)?;
    // 用户要求原始布局保留文件行高：Excel 的 pt 按 96/72 转 px，最大 2048px；
    // 先限制源数值再换算，避免异常大值溢出；前端须同步允许 1–2048px。
    Some(
        (height.min(f64::from(MAX_ROW_HEIGHT) * 0.75) * 4.0 / 3.0)
            .round()
            .clamp(1.0, f64::from(MAX_ROW_HEIGHT)) as u16,
    )
}

fn spreadsheet_layouts(zip: &mut OfficeZip<'_>) -> PreviewResult<HashMap<String, SheetLayout>> {
    let root = xml_entry(zip, "_rels/.rels")?;
    let root = xml(&root)?;
    let workbook_path = root
        .descendants()
        .find(|node| {
            element(*node, "Relationship")
                && node
                    .attribute("Type")
                    .is_some_and(|kind| kind.ends_with("/officeDocument"))
                && node.attribute("TargetMode") != Some("External")
        })
        .and_then(|node| node.attribute("Target"))
        .ok_or("Excel 工作簿关系无效")?;
    let workbook_path = package_part("", workbook_path)?;
    let (directory, name) = workbook_path
        .rsplit_once('/')
        .map_or(("", workbook_path.as_str()), |value| value);
    let relationships_path = if directory.is_empty() {
        format!("_rels/{name}.rels")
    } else {
        format!("{directory}/_rels/{name}.rels")
    };
    let relationships = xml_entry(zip, &relationships_path)?;
    let relationships = xml(&relationships)?;
    let targets: HashMap<_, _> = relationships
        .descendants()
        .filter(|node| {
            element(*node, "Relationship") && node.attribute("TargetMode") != Some("External")
        })
        .filter_map(|node| Some((node.attribute("Id")?, node.attribute("Target")?)))
        .collect();
    let workbook = xml_entry(zip, &workbook_path)?;
    let workbook = xml(&workbook)?;
    let mut layouts = HashMap::new();
    for sheet in workbook
        .descendants()
        .filter(|node| element(*node, "sheet"))
        .take(MAX_SHEETS)
    {
        let name = sheet.attribute("name").ok_or("Excel 工作表名称无效")?;
        let id = sheet
            .attributes()
            .find(|attribute| attribute.name() == "id")
            .map(|attribute| attribute.value())
            .ok_or("Excel 工作表关系无效")?;
        let path = package_part(
            &workbook_path,
            targets.get(id).ok_or("Excel 工作表关系无效")?,
        )?;
        let source = xml_entry(zip, &path)?;
        let tree = xml(&source)?;
        let mut layout = SheetLayout::default();
        // 用户要求合并效果和正确列宽：读取原文件范围与列定义，不能把合并标题当作 A 列的普通长文字。
        for merge in tree
            .descendants()
            .filter(|node| element(*node, "mergeCell"))
        {
            if layout.merges.len() >= MAX_MERGES {
                return Err("Excel 合并区域过多");
            }
            let reference = merge.attribute("ref").ok_or("Excel 合并范围无效")?;
            let (start, end) = reference.split_once(':').unwrap_or((reference, reference));
            let (start_row, start_column) = cell_position(start)?;
            let (end_row, end_column) = cell_position(end)?;
            if start_row > end_row || start_column > end_column {
                return Err("Excel 合并范围无效");
            }
            layout.merges.push(MergeRange {
                start_row,
                end_row,
                start_column,
                end_column,
            });
        }
        let format = tree
            .descendants()
            .find(|node| element(*node, "sheetFormatPr"));
        layout.default_row_height =
            spreadsheet_row_height(format.and_then(|node| node.attribute("defaultRowHeight")));
        let default_width =
            spreadsheet_column_width(format.and_then(|node| node.attribute("defaultColWidth")))
                .unwrap_or(DEFAULT_COLUMN_WIDTH);
        layout.widths.resize(MAX_COLUMNS, default_width);
        for column in tree.descendants().filter(|node| element(*node, "col")) {
            let start = column
                .attribute("min")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            let end = column
                .attribute("max")
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0)
                .min(MAX_COLUMNS);
            let Some(width) = spreadsheet_column_width(column.attribute("width")) else {
                continue;
            };
            if start > 0 && start <= end {
                for value in &mut layout.widths[start - 1..end] {
                    *value = width;
                }
            }
        }
        // 行高按真实行号稀疏记录，不能按百万行声明展开；元数据数量也受总格数上限约束。
        for row in tree.descendants().filter(|node| element(*node, "row")) {
            let number = row
                .attribute("r")
                .and_then(|value| value.parse::<u32>().ok())
                .filter(|number| (1..=1_048_576).contains(number));
            let height = spreadsheet_row_height(row.attribute("ht"));
            if let (Some(number), Some(height)) = (number, height) {
                if layout.row_heights.len() >= MAX_CELLS
                    && !layout.row_heights.contains_key(&number)
                {
                    return Err("Excel 行高定义过多，暂不提供在线预览");
                }
                layout.row_heights.insert(number, height);
            }
        }
        layouts.insert(name.to_owned(), layout);
    }
    Ok(layouts)
}

fn apply_sheet_layout(
    sheet: &mut Sheet,
    layout: SheetLayout,
    budget: &mut Budget,
) -> PreviewResult<()> {
    let mut rows: BTreeMap<_, _> = std::mem::take(&mut sheet.rows)
        .into_iter()
        .map(|row| (row.number, row.cells))
        .collect();
    let last_row = rows
        .last_key_value()
        .map(|(number, _)| *number)
        .unwrap_or(0);
    // 普通小表保留中间空行；巨大稀疏表仍保持真实行号，不能按百万行声明展开。
    if last_row <= MAX_ROWS as u32 && last_row > 0 {
        for number in 1..=last_row {
            rows.entry(number).or_default();
        }
    }
    let mut occupied = HashSet::new();
    for mut merge in layout.merges {
        if merge.start_column >= MAX_COLUMNS || !rows.contains_key(&merge.start_row) {
            continue;
        }
        if merge.end_row.saturating_sub(merge.start_row) >= MAX_ROWS as u32 {
            sheet.truncated = true;
            continue;
        }
        let additional = (merge.start_row..=merge.end_row)
            .filter(|number| !rows.contains_key(number))
            .count();
        if rows.len() + additional > MAX_ROWS {
            sheet.truncated = true;
            continue;
        }
        if merge.end_column >= MAX_COLUMNS {
            merge.end_column = MAX_COLUMNS - 1;
            sheet.truncated = true;
        }
        for row in merge.start_row..=merge.end_row {
            rows.entry(row).or_default();
            for column in merge.start_column..=merge.end_column {
                if !occupied.insert((row, column)) {
                    return Err("Excel 合并区域重叠，暂不提供预览");
                }
            }
        }
        sheet.column_count = sheet.column_count.max(merge.end_column + 1);
        sheet.merges.push(merge);
    }
    sheet.rows = rows
        .into_iter()
        .map(|(number, cells)| Row {
            number,
            cells,
            height: layout.row_heights.get(&number).copied(),
            style_ids: Vec::new(),
        })
        .collect();
    sheet.column_widths = (0..sheet.column_count)
        .map(|index| {
            layout
                .widths
                .get(index)
                .copied()
                .unwrap_or(DEFAULT_COLUMN_WIDTH)
        })
        .collect();
    // XLSX 缺省行高按 15pt = 20px 还原；CSV/TSV/ODS 仍省略行高字段，由前端选择默认布局。
    sheet.default_row_height = Some(layout.default_row_height.unwrap_or(DEFAULT_ROW_HEIGHT));
    budget.truncated |= sheet.truncated;
    Ok(())
}

fn spreadsheet(bytes: &[u8]) -> PreviewResult<Value> {
    let mut zip = office_zip(bytes)?;
    // 原生读取器会按 sharedStrings 的 uniqueCount 预分配；先校验实际 XML 和声明，
    // 不能让很小的文件仅凭伪造计数申请巨量内存。
    let shared_strings: Vec<_> = zip
        .file_names()
        .filter(|name| name.ends_with("/sharedStrings.xml"))
        .map(str::to_owned)
        .collect();
    for path in shared_strings {
        let source = xml_entry(&mut zip, &path)?;
        let tree = xml(&source)?;
        if tree
            .root_element()
            .attribute("uniqueCount")
            .is_some_and(|value| value.parse::<usize>().map_or(true, |count| count > 300_000))
        {
            return Err("Excel 共享文本数量过多或声明无效，暂不提供在线预览");
        }
    }
    let mut layouts = spreadsheet_layouts(&mut zip)?;
    drop(zip);
    let mut workbook: Xlsx<_> = calamine::open_workbook_from_rs(Cursor::new(bytes))
        .map_err(|_| "Excel 文件无法读取，可能已损坏或已加密")?;
    let names = workbook.sheet_names();
    let mut budget = Budget::default();
    let mut sheets = Vec::new();
    for name in names.iter().take(MAX_SHEETS) {
        let mut reader = workbook
            .worksheet_cells_reader(name)
            .map_err(|_| "Excel 工作表无法读取")?;
        let mut sheet = SheetBuilder::default();
        let mut visited = 0;
        // 用原生逐格流式读取，不能按声明的巨大稀疏区域申请整张矩形表。
        while let Some(cell) = reader.next_cell().map_err(|_| "Excel 单元格内容无效")? {
            visited += 1;
            if visited > MAX_CELLS || budget.bytes >= MAX_TEXT {
                sheet.truncated = true;
                break;
            }
            let (row, column) = cell.get_position();
            let text = cell_text(cell.get_value());
            if !sheet.insert(row.saturating_add(1), column as usize, &text, &mut budget)
                && (sheet.rows.len() >= MAX_ROWS
                    || budget.cells >= MAX_CELLS
                    || budget.bytes >= MAX_TEXT)
            {
                break;
            }
        }
        drop(reader);
        budget.truncated |= sheet.truncated;
        let mut sheet = sheet.finish(name.chars().take(256).collect());
        apply_sheet_layout(
            &mut sheet,
            layouts.remove(name).unwrap_or_default(),
            &mut budget,
        )?;
        // 用户要求保留背景、字号与文字效果；按绝对坐标读取稀疏样式，合并与补空行不能改变样式归属。
        let style_cells = sheet.rows.len().saturating_mul(sheet.column_count);
        if budget.style_cells + style_cells > MAX_CELLS {
            sheet.truncated = true;
            budget.truncated = true;
            sheets.push(sheet);
            continue;
        }
        budget.style_cells += style_cells;
        let styles = workbook
            .worksheet_style(name)
            .map_err(|_| "Excel 单元格样式无法读取")?;
        let mut palette = HashMap::new();
        for row in &mut sheet.rows {
            for column in 0..sheet.column_count {
                let format = styles
                    .get((row.number - 1, column as u32))
                    .map(cell_style)
                    .unwrap_or_else(|| CellStyle {
                        horizontal: "left",
                        vertical: "top",
                        ..CellStyle::default()
                    });
                let key = serde_json::to_string(&format).map_err(|_| "Excel 单元格样式无效")?;
                let id = if let Some(id) = palette.get(&key) {
                    *id
                } else {
                    let id = sheet.styles.len();
                    if id >= 4096 {
                        return Err("Excel 单元格样式过多");
                    }
                    palette.insert(key, id);
                    sheet.styles.push(format);
                    id
                };
                row.style_ids.push(id);
            }
        }
        sheets.push(sheet);
    }
    budget.truncated |= names.len() > MAX_SHEETS;
    Ok(spreadsheet_value(sheets, budget.truncated))
}

fn cell_text(value: &DataRef<'_>) -> String {
    match value {
        DataRef::DateTime(value) if value.is_datetime() => {
            let (year, month, day, hour, minute, second, millis) = value.to_ymd_hms_milli();
            if hour == 0 && minute == 0 && second == 0 && millis == 0 {
                format!("{year:04}-{month:02}-{day:02}")
            } else {
                format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
            }
        }
        _ => calamine::Data::from(value.clone()).to_string(),
    }
}

fn delimited(bytes: &[u8], delimiter: u8) -> PreviewResult<Value> {
    let text = std::str::from_utf8(bytes).map_err(|_| "CSV / TSV 请使用 UTF-8 编码后在线查看")?;
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .from_reader(text.trim_start_matches('\u{feff}').as_bytes());
    let mut budget = Budget::default();
    let mut sheet = SheetBuilder::default();
    for (index, record) in reader.records().enumerate() {
        let record = record.map_err(|_| "表格分隔符或引号格式无效")?;
        if index >= MAX_ROWS || budget.bytes >= MAX_TEXT || budget.cells >= MAX_CELLS {
            sheet.truncated = true;
            break;
        }
        for (column, text) in record.iter().enumerate() {
            if !sheet.insert(index as u32 + 1, column, text, &mut budget) {
                break;
            }
        }
    }
    let truncated = budget.truncated || sheet.truncated;
    Ok(spreadsheet_value(
        vec![sheet.finish("工作表".into())],
        truncated,
    ))
}

fn spreadsheet_value(sheets: Vec<Sheet>, truncated: bool) -> Value {
    json!({
        "kind": "spreadsheet", "sheets": sheets, "truncated": truncated,
        "message": if truncated { Some("内容较多，仅预览前 20 张工作表、每表 1000 行 / 100 列及限额内内容") } else { None }
    })
}

fn element(node: Node<'_, '_>, name: &str) -> bool {
    node.is_element() && node.tag_name().name() == name
}

fn paragraph_text(node: Node<'_, '_>) -> String {
    let mut text = String::new();
    for child in node.descendants() {
        if child.is_text() {
            text.push_str(child.text().unwrap_or(""));
        } else if element(child, "tab") {
            text.push('\t');
        } else if element(child, "br") || element(child, "line-break") {
            text.push('\n');
        }
        if text.len() > MAX_CELL_TEXT {
            break;
        }
    }
    text
}

fn collect_blocks(node: Node<'_, '_>, blocks: &mut Vec<Block>, budget: &mut Budget, depth: usize) {
    if depth > 64 {
        budget.truncated = true;
        return;
    }
    for child in node.children().filter(Node::is_element) {
        if budget.blocks >= MAX_BLOCKS || budget.bytes >= MAX_TEXT {
            budget.truncated = true;
            break;
        }
        if element(child, "p") || element(child, "h") {
            let text = budget.text(&paragraph_text(child));
            let heading = element(child, "h")
                || child.descendants().any(|value| {
                    element(value, "pStyle")
                        && value.attributes().any(|attribute| {
                            attribute.name() == "val"
                                && attribute
                                    .value()
                                    .to_ascii_lowercase()
                                    .starts_with("heading")
                        })
                });
            budget.blocks += 1;
            blocks.push(if heading {
                Block::Heading { text }
            } else {
                Block::Paragraph { text }
            });
        } else if element(child, "tbl") || element(child, "table") {
            let mut rows = Vec::new();
            for row in child
                .children()
                .filter(|value| element(*value, "tr") || element(*value, "table-row"))
                .take(MAX_ROWS + 1)
            {
                if rows.len() >= MAX_ROWS || budget.cells >= MAX_CELLS || budget.bytes >= MAX_TEXT {
                    budget.truncated = true;
                    break;
                }
                let mut cells = Vec::new();
                for cell in row
                    .children()
                    .filter(|value| element(*value, "tc") || element(*value, "table-cell"))
                {
                    if cells.len() >= MAX_COLUMNS || budget.cells >= MAX_CELLS {
                        budget.truncated = true;
                        break;
                    }
                    budget.cells += 1;
                    let text = cell
                        .descendants()
                        .filter(|value| element(*value, "p"))
                        .map(paragraph_text)
                        .collect::<Vec<_>>()
                        .join("\n");
                    cells.push(budget.text(&text));
                }
                rows.push(cells);
            }
            budget.blocks += 1;
            blocks.push(Block::Table { rows });
        } else {
            collect_blocks(child, blocks, budget, depth + 1);
        }
    }
}

fn document(bytes: &[u8], format: &str) -> PreviewResult<Value> {
    let mut zip = office_zip(bytes)?;
    let source = xml_entry(
        &mut zip,
        if format == "docx" {
            "word/document.xml"
        } else {
            "content.xml"
        },
    )?;
    let tree = xml(&source)?;
    let body = tree
        .descendants()
        .find(|node| element(*node, if format == "docx" { "body" } else { "text" }))
        .ok_or("文档缺少正文")?;
    let mut budget = Budget::default();
    let mut blocks = Vec::new();
    collect_blocks(body, &mut blocks, &mut budget, 0);
    Ok(document_value(
        vec![Section {
            title: String::new(),
            blocks,
        }],
        budget.truncated,
    ))
}

fn repeat(node: Node<'_, '_>, name: &str) -> usize {
    node.attributes()
        .find(|attribute| attribute.name() == name)
        .and_then(|attribute| attribute.value().parse::<usize>().ok())
        .unwrap_or(1)
        .max(1)
}

fn open_document_sheets(bytes: &[u8]) -> PreviewResult<Value> {
    let mut zip = office_zip(bytes)?;
    let source = xml_entry(&mut zip, "content.xml")?;
    let tree = xml(&source)?;
    let tables: Vec<_> = tree
        .descendants()
        .filter(|node| element(*node, "table"))
        .collect();
    let mut budget = Budget::default();
    let mut sheets = Vec::new();
    for (index, table) in tables.iter().take(MAX_SHEETS).enumerate() {
        let mut sheet = SheetBuilder::default();
        let mut row_number = 1_u32;
        for row in table.children().filter(|node| element(*node, "table-row")) {
            let repetitions = repeat(row, "number-rows-repeated");
            let mut values = Vec::new();
            let mut column = 0_usize;
            for cell in row
                .children()
                .filter(|node| element(*node, "table-cell") || element(*node, "covered-table-cell"))
            {
                let text = cell
                    .descendants()
                    .filter(|node| element(*node, "p"))
                    .map(paragraph_text)
                    .collect::<Vec<_>>()
                    .join("\n");
                let text = if text.is_empty() {
                    cell.attributes()
                        .find(|attribute| {
                            matches!(attribute.name(), "string-value" | "date-value" | "value")
                        })
                        .map(|attribute| attribute.value())
                        .unwrap_or("")
                        .to_owned()
                } else {
                    text
                };
                let count = repeat(cell, "number-columns-repeated");
                if !text.is_empty() {
                    for offset in 0..count.min(MAX_COLUMNS + 1) {
                        if column.saturating_add(offset) >= MAX_COLUMNS {
                            sheet.truncated = true;
                            break;
                        }
                        values.push((column + offset, text.clone()));
                    }
                }
                column = column.saturating_add(count);
            }
            // ODS 经常用重复空行/空列补齐整张纸；只推进真实行号，不能因此丢掉下一行或展开巨量空格。
            if values.is_empty() {
                row_number = row_number.saturating_add(repetitions.min(u32::MAX as usize) as u32);
                continue;
            }
            for _ in 0..repetitions.min(MAX_ROWS + 1) {
                if sheet.rows.len() >= MAX_ROWS
                    || budget.bytes >= MAX_TEXT
                    || budget.cells >= MAX_CELLS
                {
                    sheet.truncated = true;
                    break;
                }
                for (column, text) in &values {
                    if !sheet.insert(row_number, *column, text, &mut budget) {
                        break;
                    }
                }
                row_number = row_number.saturating_add(1);
            }
            if repetitions > MAX_ROWS
                || sheet.rows.len() >= MAX_ROWS
                || budget.cells >= MAX_CELLS
                || budget.bytes >= MAX_TEXT
            {
                sheet.truncated = true;
                break;
            }
        }
        budget.truncated |= sheet.truncated;
        let name = table
            .attributes()
            .find(|attribute| attribute.name() == "name")
            .map(|attribute| attribute.value().to_owned())
            .unwrap_or_else(|| format!("工作表 {}", index + 1));
        sheets.push(sheet.finish(name.chars().take(256).collect()));
    }
    budget.truncated |= tables.len() > MAX_SHEETS;
    Ok(spreadsheet_value(sheets, budget.truncated))
}

fn presentation(bytes: &[u8]) -> PreviewResult<Value> {
    let mut zip = office_zip(bytes)?;
    let source = xml_entry(&mut zip, "ppt/presentation.xml")?;
    let tree = xml(&source)?;
    let rels = xml_entry(&mut zip, "ppt/_rels/presentation.xml.rels")?;
    let rels = xml(&rels)?;
    let targets: HashMap<_, _> = rels
        .descendants()
        .filter(|node| element(*node, "Relationship"))
        .filter(|node| {
            node.attribute("TargetMode") != Some("External")
                && node
                    .attribute("Type")
                    .is_some_and(|value| value.ends_with("/slide"))
        })
        .filter_map(|node| Some((node.attribute("Id")?, node.attribute("Target")?)))
        .collect();
    let ids: Vec<_> = tree
        .descendants()
        .filter(|node| element(*node, "sldId"))
        .filter_map(|node| {
            node.attributes()
                .find(|attribute| attribute.name() == "id" && attribute.namespace().is_some())
                .map(|attribute| attribute.value())
        })
        .collect();
    let mut budget = Budget::default();
    let mut sections = Vec::new();
    for (index, id) in ids.iter().take(MAX_SECTIONS).enumerate() {
        let target = targets.get(id).ok_or("幻灯片内容关系无效")?;
        let path = if target.starts_with('/') {
            target.trim_start_matches('/').to_string()
        } else {
            format!("ppt/{target}")
        };
        if !path.starts_with("ppt/slides/")
            || path.split('/').any(|part| part == ".." || part == ".")
        {
            return Err("幻灯片内容路径无效");
        }
        let source = xml_entry(&mut zip, &path)?;
        let tree = xml(&source)?;
        let mut blocks = Vec::new();
        for paragraph in tree.descendants().filter(|node| element(*node, "p")) {
            if budget.blocks >= MAX_BLOCKS || budget.bytes >= MAX_TEXT {
                budget.truncated = true;
                break;
            }
            budget.blocks += 1;
            blocks.push(Block::Paragraph {
                text: budget.text(&paragraph_text(paragraph)),
            });
        }
        sections.push(Section {
            title: format!("第 {} 页", index + 1),
            blocks,
        });
        if budget.truncated {
            break;
        }
    }
    budget.truncated |= ids.len() > MAX_SECTIONS;
    Ok(document_value(sections, budget.truncated))
}

struct PdfText {
    bytes: Vec<u8>,
    remaining: usize,
    truncated: bool,
}

impl Write for PdfText {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.remaining {
            self.truncated = true;
            return Err(std::io::ErrorKind::FileTooLarge.into());
        }
        self.bytes.extend_from_slice(bytes);
        self.remaining -= bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn pdf(bytes: &[u8]) -> PreviewResult<Value> {
    let document =
        pdf_extract::Document::load_mem(bytes).map_err(|_| "PDF 无法读取，可能已损坏")?;
    if document.is_encrypted() {
        return Err("加密 PDF 请先解密后在线查看");
    }
    let pages = document.get_pages();
    let mut remaining = MAX_TEXT;
    let mut truncated = pages.len() > MAX_SECTIONS;
    let mut sections = Vec::new();
    for number in pages.keys().take(MAX_SECTIONS) {
        let mut text = PdfText {
            bytes: Vec::new(),
            remaining,
            truncated: false,
        };
        let result = {
            let mut output = pdf_extract::PlainTextOutput::new(&mut text as &mut dyn Write);
            pdf_extract::output_doc_page(&document, &mut output, *number)
        };
        if result.is_err() && !text.truncated {
            return Err("PDF 文字无法解析，可能为扫描件或使用了不支持的字体");
        }
        let content = String::from_utf8(text.bytes).map_err(|_| "PDF 文字编码无效")?;
        remaining = text.remaining;
        truncated |= text.truncated;
        sections.push(Section {
            title: format!("第 {number} 页"),
            blocks: vec![Block::Paragraph { text: content }],
        });
        if text.truncated || remaining == 0 {
            truncated |= remaining == 0;
            break;
        }
    }
    let mut value = document_value(sections, truncated);
    if value["sections"].as_array().is_some_and(|sections| {
        sections.iter().all(|section| {
            section["blocks"].as_array().is_none_or(|blocks| {
                blocks
                    .iter()
                    .all(|block| block["text"].as_str().unwrap_or("").trim().is_empty())
            })
        })
    }) {
        value["message"] = json!("此 PDF 没有可提取的文字，可能是扫描件，暂不支持 OCR 识别");
    }
    Ok(value)
}

fn document_value(sections: Vec<Section>, truncated: bool) -> Value {
    // 用户要求移除常驻正文预览说明；仍保留超限提示，PDF 无可提取文字时另行说明。
    json!({
        "kind": "document", "sections": sections, "truncated": truncated,
        "message": if truncated { Some("文档较长，仅显示限额内的正文内容") }
            else { None }
    })
}
