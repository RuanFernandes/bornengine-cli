use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

const ENGINE_PACKAGE: &str = "@bornengine/engine";
const COLYSEUS_SUBPATH: &str = "@bornengine/engine/colyseus";
const COLYSEUS_EXPORTS: [&str; 2] = ["ColyseusClient", "Room"];
const DIALOG_FUNCTIONS: [&str; 2] = ["openFileDialog", "saveFileDialog"];
const SKIPPED_DIRECTORIES: [&str; 3] = ["node_modules", "dist", "build"];
const SOURCE_EXTENSIONS: [&str; 4] = ["ts", "tsx", "mts", "cts"];
const MAX_STATEMENT_TOKENS: usize = 400;

pub const MULTIPLAYER: &str = "multiplayer";
pub const DIALOGS: &str = "dialogs";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectedFeature {
    pub name: &'static str,
    pub file: PathBuf,
    pub line: usize,
}

/// Finds optional native engine features that the project's TypeScript uses.
///
/// The scan is lexical, not a full parse. Its failure mode is enabling a feature
/// that is not needed, which costs binary size but never breaks a build.
pub fn detect_native_features(project_root: &Path) -> Result<Vec<DetectedFeature>> {
    let mut found: Vec<DetectedFeature> = Vec::new();
    for path in source_files(project_root)? {
        let source = fs::read_to_string(&path)
            .with_context(|| format!("could not read {}", path.display()))?;
        let relative = path.strip_prefix(project_root).unwrap_or(&path);
        for (name, line) in detect_in_source(&source) {
            if !found.iter().any(|existing| existing.name == name) {
                found.push(DetectedFeature {
                    name,
                    file: relative.to_path_buf(),
                    line,
                });
            }
        }
        if found.len() == 2 {
            break;
        }
    }
    Ok(found)
}

fn source_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory)
            .with_context(|| format!("could not read directory {}", directory.display()))?;
        for entry in entries {
            let entry = entry
                .with_context(|| format!("could not read entry in {}", directory.display()))?;
            let file_type = entry
                .file_type()
                .with_context(|| format!("could not inspect {}", entry.path().display()))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if file_type.is_dir() {
                if !name.starts_with('.') && !SKIPPED_DIRECTORIES.contains(&name.as_str()) {
                    pending.push(path);
                }
            } else if file_type.is_file() && is_source_file(&name) {
                files.push(path);
            }
        }
    }
    files.sort();
    Ok(files)
}

fn is_source_file(name: &str) -> bool {
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    SOURCE_EXTENSIONS.contains(&extension) && !stem.ends_with(".d")
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Identifier(String),
    Text(String),
    Punctuation(char),
}

struct Positioned {
    token: Token,
    line: usize,
}

fn detect_in_source(source: &str) -> Vec<(&'static str, usize)> {
    let tokens = tokenize(source);
    let mut found: Vec<(&'static str, usize)> = Vec::new();
    let mut record = |name: &'static str, line: usize| {
        if !found.iter().any(|(existing, _)| *existing == name) {
            found.push((name, line));
        }
    };

    let mut engine_namespaces: Vec<String> = Vec::new();
    for (index, positioned) in tokens.iter().enumerate() {
        match &positioned.token {
            Token::Identifier(word) if DIALOG_FUNCTIONS.contains(&word.as_str()) => {
                record(DIALOGS, positioned.line);
            }
            Token::Identifier(word) if word == "import" || word == "export" => {
                if let Some(statement) = parse_module_statement(&tokens, index) {
                    match statement.specifier.as_str() {
                        COLYSEUS_SUBPATH => record(MULTIPLAYER, positioned.line),
                        ENGINE_PACKAGE => {
                            if statement.star_reexport
                                || statement
                                    .named
                                    .iter()
                                    .any(|name| COLYSEUS_EXPORTS.contains(&name.as_str()))
                            {
                                record(MULTIPLAYER, positioned.line);
                            }
                            engine_namespaces.extend(statement.namespace);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    for namespace in &engine_namespaces {
        if let Some(line) = namespace_member_use(&tokens, namespace) {
            record(MULTIPLAYER, line);
        }
    }
    found
}

struct ModuleStatement {
    specifier: String,
    named: Vec<String>,
    namespace: Option<String>,
    star_reexport: bool,
}

/// Reads the import or export statement starting at `start`, or a dynamic
/// `import("...")`. Returns `None` for exports that are not re-exports.
fn parse_module_statement(tokens: &[Positioned], start: usize) -> Option<ModuleStatement> {
    let is_export = matches!(&tokens[start].token, Token::Identifier(word) if word == "export");
    let mut statement = ModuleStatement {
        specifier: String::new(),
        named: Vec::new(),
        namespace: None,
        star_reexport: false,
    };

    match tokens.get(start + 1).map(|next| &next.token) {
        Some(Token::Text(specifier)) if !is_export => {
            statement.specifier = specifier.clone();
            return Some(statement);
        }
        Some(Token::Punctuation('(')) if !is_export => {
            if let Some(Token::Text(specifier)) = tokens.get(start + 2).map(|next| &next.token) {
                statement.specifier = specifier.clone();
                return Some(statement);
            }
            return None;
        }
        _ => {}
    }

    let mut index = start + 1;
    let mut brace_depth = 0usize;
    let mut braced: Vec<String> = Vec::new();
    let mut item: Vec<String> = Vec::new();
    while index < tokens.len() && index - start <= MAX_STATEMENT_TOKENS {
        match &tokens[index].token {
            Token::Punctuation('{') => brace_depth += 1,
            Token::Punctuation('}') => {
                if brace_depth == 1 {
                    flush_item(&mut item, &mut braced);
                }
                brace_depth = brace_depth.saturating_sub(1);
            }
            Token::Punctuation(',') if brace_depth == 1 => flush_item(&mut item, &mut braced),
            Token::Punctuation(';' | '=' | '(' | ')') if brace_depth == 0 => return None,
            Token::Identifier(word) if brace_depth > 0 => item.push(word.clone()),
            Token::Identifier(word) if brace_depth == 0 => {
                if word == "from" {
                    if let Some(Token::Text(specifier)) = tokens.get(index + 1).map(|t| &t.token) {
                        statement.specifier = specifier.clone();
                        statement.named = braced;
                        return Some(statement);
                    }
                    return None;
                }
                if matches!(
                    word.as_str(),
                    "const"
                        | "let"
                        | "var"
                        | "function"
                        | "class"
                        | "interface"
                        | "enum"
                        | "default"
                        | "async"
                        | "abstract"
                        | "declare"
                        | "namespace"
                        | "module"
                ) {
                    return None;
                }
                if word == "as" {
                    if let Some(Token::Identifier(alias)) = tokens.get(index + 1).map(|t| &t.token)
                    {
                        if matches!(
                            index.checked_sub(1).map(|prior| &tokens[prior].token),
                            Some(Token::Punctuation('*'))
                        ) {
                            statement.namespace = Some(alias.clone());
                            if is_export {
                                statement.star_reexport = true;
                            }
                        }
                    }
                }
            }
            Token::Punctuation('*') if brace_depth == 0 => {
                let renamed = matches!(
                    tokens.get(index + 1).map(|next| &next.token),
                    Some(Token::Identifier(word)) if word == "as"
                );
                if is_export && !renamed {
                    statement.star_reexport = true;
                }
            }
            _ => {}
        }
        index += 1;
    }
    None
}

fn flush_item(item: &mut Vec<String>, names: &mut Vec<String>) {
    let imported = match item.as_slice() {
        [first, second, ..] if first == "type" && second != "as" => Some(second),
        [first, ..] => Some(first),
        [] => None,
    };
    if let Some(name) = imported {
        names.push(name.clone());
    }
    item.clear();
}

fn namespace_member_use(tokens: &[Positioned], namespace: &str) -> Option<usize> {
    tokens.iter().enumerate().find_map(|(index, positioned)| {
        if !matches!(&positioned.token, Token::Identifier(word) if word == namespace) {
            return None;
        }
        let mut cursor = index + 1;
        if matches!(
            tokens.get(cursor).map(|next| &next.token),
            Some(Token::Punctuation('?'))
        ) {
            cursor += 1;
        }
        if !matches!(
            tokens.get(cursor).map(|next| &next.token),
            Some(Token::Punctuation('.'))
        ) {
            return None;
        }
        match tokens.get(cursor + 1).map(|next| &next.token) {
            Some(Token::Identifier(member)) if COLYSEUS_EXPORTS.contains(&member.as_str()) => {
                Some(positioned.line)
            }
            _ => None,
        }
    })
}

/// Splits source into identifiers, string literals and single punctuation
/// characters, dropping comments. Newlines inside comments and template
/// literals are counted so reported line numbers stay accurate.
fn tokenize(source: &str) -> Vec<Positioned> {
    let characters: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut line = 1usize;
    let mut index = 0usize;
    while index < characters.len() {
        let character = characters[index];
        let next = characters.get(index + 1).copied();
        match character {
            '\n' => {
                line += 1;
                index += 1;
            }
            _ if character.is_whitespace() => index += 1,
            '/' if next == Some('/') => {
                while index < characters.len() && characters[index] != '\n' {
                    index += 1;
                }
            }
            '/' if next == Some('*') => {
                index += 2;
                while index < characters.len()
                    && !(characters[index] == '*' && characters.get(index + 1) == Some(&'/'))
                {
                    if characters[index] == '\n' {
                        line += 1;
                    }
                    index += 1;
                }
                index = (index + 2).min(characters.len());
            }
            '\'' | '"' | '`' => {
                let start_line = line;
                let mut text = String::new();
                index += 1;
                while index < characters.len() && characters[index] != character {
                    if characters[index] == '\\' && index + 1 < characters.len() {
                        text.push(characters[index + 1]);
                        index += 2;
                        continue;
                    }
                    if characters[index] == '\n' {
                        line += 1;
                        // Only template literals may span lines; recover from an
                        // unbalanced quote (for example inside a regex literal).
                        if character != '`' {
                            break;
                        }
                    }
                    text.push(characters[index]);
                    index += 1;
                }
                index += 1;
                tokens.push(Positioned {
                    token: Token::Text(text),
                    line: start_line,
                });
            }
            _ if character.is_alphabetic() || character == '_' || character == '$' => {
                let start = index;
                while index < characters.len()
                    && (characters[index].is_alphanumeric()
                        || characters[index] == '_'
                        || characters[index] == '$')
                {
                    index += 1;
                }
                tokens.push(Positioned {
                    token: Token::Identifier(characters[start..index].iter().collect()),
                    line,
                });
            }
            _ => {
                tokens.push(Positioned {
                    token: Token::Punctuation(character),
                    line,
                });
                index += 1;
            }
        }
    }
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(source: &str) -> Vec<&'static str> {
        detect_in_source(source)
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    #[test]
    fn colyseus_subpath_import_enables_multiplayer() {
        assert_eq!(
            names("import { ColyseusClient } from \"@bornengine/engine/colyseus\";"),
            [MULTIPLAYER]
        );
        assert_eq!(
            names("import '@bornengine/engine/colyseus';"),
            [MULTIPLAYER]
        );
        assert_eq!(
            names("const m = await import(\"@bornengine/engine/colyseus\");"),
            [MULTIPLAYER]
        );
    }

    #[test]
    fn named_import_from_root_enables_multiplayer() {
        assert_eq!(
            names("import { Game, ColyseusClient } from '@bornengine/engine';"),
            [MULTIPLAYER]
        );
        assert_eq!(
            names("import { Game, Room } from '@bornengine/engine';"),
            [MULTIPLAYER]
        );
        assert_eq!(
            names("import { type Room } from '@bornengine/engine';"),
            [MULTIPLAYER]
        );
        assert_eq!(
            names("export { ColyseusClient } from '@bornengine/engine';"),
            [MULTIPLAYER]
        );
    }

    #[test]
    fn aliased_import_uses_the_original_name() {
        assert_eq!(
            names("import { Room as R } from '@bornengine/engine';"),
            [MULTIPLAYER]
        );
        assert!(names("import { Game as Room } from '@bornengine/engine';").is_empty());
    }

    #[test]
    fn multiline_import_is_read_as_one_statement() {
        let source =
            "import {\n  Game,\n  Vector2D,\n  ColyseusClient,\n} from '@bornengine/engine';\n";
        assert_eq!(detect_in_source(source), [(MULTIPLAYER, 1)]);
    }

    #[test]
    fn namespace_import_needs_a_member_access() {
        let used = "import * as E from '@bornengine/engine';\nconst c = new E.ColyseusClient(game, url);\n";
        assert_eq!(detect_in_source(used), [(MULTIPLAYER, 2)]);
        let optional = "import * as E from '@bornengine/engine';\nE?.Room;\n";
        assert_eq!(names(optional), [MULTIPLAYER]);
        let unused = "import * as E from '@bornengine/engine';\nnew E.Game({});\n";
        assert!(names(unused).is_empty());
    }

    #[test]
    fn star_reexport_of_the_engine_enables_multiplayer() {
        assert_eq!(names("export * from '@bornengine/engine';"), [MULTIPLAYER]);
        assert_eq!(
            names("export * as engine from '@bornengine/engine';"),
            [MULTIPLAYER]
        );
    }

    #[test]
    fn importing_only_the_game_enables_nothing() {
        assert!(names("import { Game } from '@bornengine/engine';\nnew Game({});").is_empty());
        assert!(names("import { Game } from '@bornengine/engine/core';").is_empty());
    }

    #[test]
    fn comments_do_not_count() {
        let source = "// import { Room } from '@bornengine/engine';\n/* this.input.openFileDialog('', '') */\nconst a = 1;\n";
        assert!(names(source).is_empty());
    }

    #[test]
    fn strings_do_not_count_as_identifiers() {
        assert!(names("const label = 'openFileDialog';").is_empty());
    }

    #[test]
    fn room_from_another_package_does_not_count() {
        assert!(names("import { Room } from 'colyseus.js';").is_empty());
        assert!(names("export const Room = 1;").is_empty());
    }

    #[test]
    fn file_dialog_calls_enable_dialogs() {
        assert_eq!(
            names("const path = this.input.openFileDialog('png', 'Open');"),
            [DIALOGS]
        );
        assert_eq!(names("input.saveFileDialog('a.json', 'Save');"), [DIALOGS]);
    }

    #[test]
    fn reported_line_follows_multiline_comments() {
        let source = "/*\n one\n two\n*/\ninput.openFileDialog('', '');\n";
        assert_eq!(detect_in_source(source), [(DIALOGS, 5)]);
    }

    #[test]
    fn unbalanced_quote_does_not_hide_later_statements() {
        let source = "const pattern = /'/;\nimport { Room } from '@bornengine/engine';\n";
        assert_eq!(names(source), [MULTIPLAYER]);
    }

    #[test]
    fn project_scan_skips_dependencies_and_build_output() {
        let project = tempfile::tempdir().unwrap();
        for directory in [
            "src",
            "node_modules/pkg",
            ".bornengine/builds",
            "dist",
            "build",
        ] {
            fs::create_dir_all(project.path().join(directory)).unwrap();
        }
        let usage = "input.openFileDialog('', '');\n";
        for ignored in [
            "node_modules/pkg/index.ts",
            ".bornengine/builds/main.ts",
            "dist/main.ts",
            "build/main.ts",
            "src/types.d.ts",
        ] {
            fs::write(project.path().join(ignored), usage).unwrap();
        }
        assert!(detect_native_features(project.path()).unwrap().is_empty());

        fs::write(
            project.path().join("src/net.ts"),
            "\nimport { Room } from '@bornengine/engine';\n",
        )
        .unwrap();
        assert_eq!(
            detect_native_features(project.path()).unwrap(),
            [DetectedFeature {
                name: MULTIPLAYER,
                file: PathBuf::from("src/net.ts"),
                line: 2,
            }]
        );
    }

    #[test]
    fn project_scan_reports_the_first_hit_per_feature() {
        let project = tempfile::tempdir().unwrap();
        fs::write(project.path().join("a.ts"), "input.openFileDialog('', '');").unwrap();
        fs::write(project.path().join("b.ts"), "input.saveFileDialog('', '');").unwrap();
        let found = detect_native_features(project.path()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file, PathBuf::from("a.ts"));
    }

    #[test]
    fn unreadable_source_fails_with_its_path() {
        let project = tempfile::tempdir().unwrap();
        fs::write(project.path().join("bad.ts"), [0xff, 0xfe, 0xfd]).unwrap();
        let error = detect_native_features(project.path()).unwrap_err();
        assert!(error.to_string().contains("bad.ts"), "{error:#}");
    }
}
