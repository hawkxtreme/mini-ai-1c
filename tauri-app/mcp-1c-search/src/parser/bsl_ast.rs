use tree_sitter::Parser;

pub fn create_bsl_parser() -> Parser {
    let mut parser = Parser::new();
    let language: tree_sitter::Language = tree_sitter_bsl::LANGUAGE.into();
    parser.set_language(&language).expect("Error loading BSL grammar");
    parser
}

#[derive(Debug, Clone)]
pub struct BslSymbol {
    pub name: String,
    pub kind: String,    // "procedure" | "function"
    pub start_line: u32, // 1-based
    pub end_line: u32,   // 1-based
    pub is_export: bool,
}

/// Extract all procedure and function definitions from BSL source code.
pub fn extract_symbols(source: &str) -> Vec<BslSymbol> {
    let mut parser = create_bsl_parser();
    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return vec![],
    };
    let root = tree.root_node();
    let source_bytes = source.as_bytes();
    let mut symbols = Vec::new();
    traverse_for_symbols(root, source_bytes, &mut symbols);
    symbols
}

fn traverse_for_symbols(node: tree_sitter::Node, source: &[u8], symbols: &mut Vec<BslSymbol>) {
    let kind = node.kind();
    if kind == "procedure_definition" || kind == "function_definition" {
        if let Some(name_node) = node.child_by_field_name("name") {
            let name = name_node.utf8_text(source).unwrap_or("").to_string();
            if !name.is_empty() {
                let is_export = node.child_by_field_name("export").is_some();
                let start_line = node.start_position().row as u32 + 1;
                let end_line = node.end_position().row as u32 + 1;
                let sym_kind = if kind == "procedure_definition" { "procedure" } else { "function" };
                symbols.push(BslSymbol {
                    name,
                    kind: sym_kind.to_string(),
                    start_line,
                    end_line,
                    is_export,
                });
            }
        }
        // Don't traverse into procedure/function body for nested defs
        return;
    }

    let child_count = node.child_count();
    for i in 0..child_count {
        if let Some(child) = node.child(i) {
            traverse_for_symbols(child, source, symbols);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bsl_parser_initialization() {
        let mut parser = create_bsl_parser();
        let code = "Процедура Тест() КонецПроцедуры";
        let tree = parser.parse(code, None).unwrap();
        assert_eq!(tree.root_node().kind(), "source_file");
    }

    #[test]
    fn test_extract_symbols() {
        let code = "Процедура МояПроцедура() Экспорт\nКонецПроцедуры\n\nФункция МояФункция(Параметр)\n\tВозврат Параметр;\nКонецФункции\n";
        let symbols = extract_symbols(code);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "МояПроцедура");
        assert_eq!(symbols[0].kind, "procedure");
        assert!(symbols[0].is_export);
        assert_eq!(symbols[1].name, "МояФункция");
        assert_eq!(symbols[1].kind, "function");
        assert!(!symbols[1].is_export);
    }
}
