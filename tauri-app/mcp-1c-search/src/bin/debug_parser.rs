fn main() {
    println!("Testing tree_sitter_bsl LANGUAGE...");
    let language: tree_sitter::Language = tree_sitter_bsl::LANGUAGE.into();
    println!("Language loaded: {:?}", language.version());
}
