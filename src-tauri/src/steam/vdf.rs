//! Leitor do formato KeyValues em texto da Valve (`libraryfolders.vdf`, `appmanifest_*.acf`).
//! Somente leitura: o NEXSTAGE nunca escreve nos arquivos do cliente Steam.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Vdf {
    Str(String),
    Obj(BTreeMap<String, Vdf>),
}

impl Vdf {
    /// Busca sem diferenciar maiúsculas ("AppState" / "appstate").
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Obj(m) => m.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Vdf::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn entries(&self) -> Vec<(&String, &Vdf)> {
        match self {
            Vdf::Obj(m) => m.iter().collect(),
            _ => Vec::new(),
        }
    }
}

struct P<'a> {
    c: std::iter::Peekable<std::str::Chars<'a>>,
}

impl P<'_> {
    fn skip_ws(&mut self) {
        loop {
            match self.c.peek() {
                Some(ch) if ch.is_whitespace() => {
                    self.c.next();
                }
                Some('/') => {
                    // comentário "// ..." até o fim da linha
                    let mut look = self.c.clone();
                    look.next();
                    if look.peek() == Some(&'/') {
                        for ch in self.c.by_ref() {
                            if ch == '\n' {
                                break;
                            }
                        }
                    } else {
                        return;
                    }
                }
                _ => return,
            }
        }
    }
    fn token(&mut self) -> Option<String> {
        self.skip_ws();
        match self.c.peek()? {
            '"' => {
                self.c.next();
                let mut s = String::new();
                while let Some(ch) = self.c.next() {
                    match ch {
                        '"' => return Some(s),
                        '\\' => match self.c.next()? {
                            'n' => s.push('\n'),
                            't' => s.push('\t'),
                            other => s.push(other),
                        },
                        ch => s.push(ch),
                    }
                }
                Some(s)
            }
            '{' | '}' => None,
            _ => {
                let mut s = String::new();
                while let Some(&ch) = self.c.peek() {
                    if ch.is_whitespace() || ch == '{' || ch == '}' || ch == '"' {
                        break;
                    }
                    s.push(ch);
                    self.c.next();
                }
                Some(s)
            }
        }
    }
    fn object(&mut self) -> BTreeMap<String, Vdf> {
        let mut m = BTreeMap::new();
        loop {
            self.skip_ws();
            match self.c.peek() {
                None => return m,
                Some('}') => {
                    self.c.next();
                    return m;
                }
                _ => {}
            }
            let Some(key) = self.token() else {
                self.c.next(); // "{" sem chave: ignora
                continue;
            };
            self.skip_ws();
            if self.c.peek() == Some(&'{') {
                self.c.next();
                m.insert(key, Vdf::Obj(self.object()));
            } else if let Some(v) = self.token() {
                m.insert(key, Vdf::Str(v));
            }
        }
    }
}

pub fn parse(text: &str) -> Vdf {
    let text = text.trim_start_matches('\u{feff}');
    Vdf::Obj(P { c: text.chars().peekable() }.object())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_library_folders_and_manifests() {
        let lf = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"apps"		{ "228980"		"382542123" }
	}
	"1"	{ "path" "D:\\SteamLibrary" }
}"#;
        let v = parse(lf);
        let root = v.get("LibraryFolders").unwrap();
        assert_eq!(root.get("0").unwrap().str("path"), Some(r"C:\Program Files (x86)\Steam"));
        assert_eq!(root.get("1").unwrap().str("path"), Some(r"D:\SteamLibrary"));
        let acf = "\"AppState\"\n{\n\t\"appid\"\t\t\"440\"\n\t\"name\"\t\t\"Team \\\"Fortress\\\" 2\"\n\t\"StateFlags\"\t\t\"4\"\n\t// comentário\n\t\"installdir\"\t\t\"Team Fortress 2\"\n}";
        let a = parse(acf);
        let st = a.get("appstate").unwrap();
        assert_eq!(st.str("appid"), Some("440"));
        assert_eq!(st.str("name"), Some("Team \"Fortress\" 2"));
        assert_eq!(st.str("stateflags"), Some("4"));
        // formato antigo: "1" "D:\\Lib" direto como texto
        let old = parse("\"LibraryFolders\" { \"TimeNextStatsReport\" \"1\" \"1\" \"E:\\\\Jogos\" }");
        assert_eq!(old.get("libraryfolders").unwrap().str("1"), Some(r"E:\Jogos"));
        // lixo não derruba o leitor
        let _ = parse("{{{ \"a\" ");
    }
}
