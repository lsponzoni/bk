use std::collections::HashMap;
use std::fs::File;
use std::io::{stdout, Read, Write};

use crossterm::{
    cursor,
    event::{read, Event, KeyCode, KeyEvent},
    queue,
    style::Print,
    terminal,
};

struct Bk {
    cols: u16,
    chapter: Vec<String>,
    chapter_idx: usize,
    container: zip::ZipArchive<File>,
    pos: usize,
    rows: usize,
    toc: Vec<String>,
    pad: u16,
}

fn get_toc(container: &mut zip::ZipArchive<File>) -> Vec<String> {
    // container.xml -> <rootfile> -> opf -> <manifest> + <spine>
    let mut xml = String::new();
    container
        .by_name("META-INF/container.xml")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let path = doc
        .descendants()
        .find(|n| n.has_tag_name("rootfile"))
        .unwrap()
        .attribute("full-path")
        .unwrap();

    let mut xml = String::new();
    container
        .by_name(path)
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let mut manifest = HashMap::new();
    doc.root_element()
        .children()
        .find(|n| n.has_tag_name("manifest"))
        .unwrap()
        .children()
        .filter(|n| n.is_element())
        .for_each(|n| {
            manifest.insert(
                n.attribute("id").unwrap(),
                n.attribute("href").unwrap(),
            );
        });

    let path = std::path::Path::new(&path).parent().unwrap();
    doc.root_element()
        .children()
        .find(|n| n.has_tag_name("spine"))
        .unwrap()
        .children()
        .filter(|n| n.is_element())
        .map(|n| {
            let name = manifest.get(n.attribute("idref").unwrap()).unwrap();
            path.join(name).to_str().unwrap().to_string()
        })
        .collect()
}

fn wrap(text: Vec<String>, width: u16) -> Vec<String> {
    // XXX assumes a char is 1 unit wide
    let mut wrapped = Vec::with_capacity(text.len() * 2);

    for chunk in text {
        let mut start = 0;
        let mut space = 0;
        let mut line = 0;
        let mut word = 0;

        for (i, c) in chunk.char_indices() {
            line += 1;
            word += 1;
            if c == ' ' {
                space = i;
                word = 0;
            }
            if line == width {
                wrapped.push(String::from(&chunk[start..space]));
                start = space + 1;
                line = word;
            }
        }
        wrapped.push(String::from(&chunk[start..]));
    }
    wrapped
}

impl Bk {
    fn new(
        path: &str,
        cols: u16,
        rows: usize,
        chapter_idx: usize,
        pos: usize,
        pad: u16,
    ) -> Result<Self, std::io::Error> {
        let file = File::open(path)?;
        let mut container = zip::ZipArchive::new(file)?;
        let toc = get_toc(&mut container);
        let mut bk = Bk {
            chapter: Vec::new(),
            container,
            toc,
            cols,
            rows,
            chapter_idx,
            pos,
            pad,
        };
        bk.load_chapter();
        Ok(bk)
    }
    fn load_chapter(&mut self) {
        let mut text = String::new();
        self.container
            .by_name(&self.toc[self.chapter_idx])
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        let doc = roxmltree::Document::parse(&text).unwrap();

        let mut chapter = Vec::new();
        for n in doc.descendants() {
            match n.tag_name().name() {
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "p" => {
                    chapter.push(
                        n.descendants()
                            .filter(|n| n.is_text())
                            .map(|n| n.text().unwrap())
                            .collect(),
                    );
                    chapter.push(String::from(""));
                }
                _ => (),
            }
        }
        chapter.pop(); //padding
        self.chapter = wrap(chapter, self.cols - self.pad);
    }
    fn run(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Char('q') => return true,
            KeyCode::Char('p') => {
                if self.chapter_idx > 0 {
                    self.pos = 0;
                    self.chapter_idx -= 1;
                    self.load_chapter();
                }
            }
            KeyCode::Char('n') => {
                if self.chapter_idx < self.toc.len() - 1 {
                    self.pos = 0;
                    self.chapter_idx += 1;
                    self.load_chapter();
                }
            }
            KeyCode::Char('h')
            | KeyCode::Char('k')
            | KeyCode::Left
            | KeyCode::Up
            | KeyCode::PageUp => {
                if self.pos == 0 {
                    if self.chapter_idx > 0 {
                        self.chapter_idx -= 1;
                        self.load_chapter();
                    }
                } else {
                    self.pos -= self.rows;
                }
            }
            KeyCode::Right
            | KeyCode::Down
            | KeyCode::PageDown
            | KeyCode::Char('j')
            | KeyCode::Char('l')
            | KeyCode::Char(' ') => {
                if self.pos + self.rows < self.chapter.len() {
                    self.pos += self.rows;
                } else if self.chapter_idx < self.toc.len() - 1 {
                    self.chapter_idx += 1;
                    self.load_chapter();
                    self.pos = 0;
                }
            }
            _ => (),
        }
        false
    }
}

fn restore(save_path: &str) -> (String, usize, usize) {
    let save = std::fs::read_to_string(save_path).unwrap();
    let mut lines = save.lines();
    let path = lines.next().unwrap().to_string();

    if let Some(p) = std::env::args().nth(1) {
        if p != path {
            return (p, 0, 0);
        }
    }
    (
        path,
        lines.next().unwrap().to_string().parse::<usize>().unwrap(),
        lines.next().unwrap().to_string().parse::<usize>().unwrap(),
    )
}

fn main() -> crossterm::Result<()> {
    let save_path =
        format!("{}/.local/share/bk", std::env::var("HOME").unwrap());
    let (path, chapter, pos) = restore(&save_path);
    let (cols, rows) = terminal::size().unwrap();

    let mut bk = Bk::new(&path, cols, rows as usize, chapter, pos, 2)
        .unwrap_or_else(|e| {
            println!("error reading epub: {}", e);
            std::process::exit(1);
        });

    let mut stdout = stdout();
    queue!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
    terminal::enable_raw_mode()?;

    loop {
        queue!(
            stdout,
            terminal::Clear(terminal::ClearType::All),
            cursor::MoveTo(bk.pad, 0),
        )?;

        let end = std::cmp::min(bk.pos + bk.rows, bk.chapter.len());
        for line in bk.pos..end {
            queue!(
                stdout,
                Print(&bk.chapter[line]),
                cursor::MoveToNextLine(1),
                cursor::MoveRight(bk.pad)
            )?;
        }
        stdout.flush()?;

        if let Event::Key(KeyEvent { code, .. }) = read()? {
            if bk.run(code) {
                break;
            }
        }
    }

    std::fs::write(
        save_path,
        format!("{}\n{}\n{}", path, bk.chapter_idx, bk.pos),
    )
    .unwrap();
    queue!(stdout, terminal::LeaveAlternateScreen, cursor::Show)?;
    stdout.flush()?;
    terminal::disable_raw_mode()
}
