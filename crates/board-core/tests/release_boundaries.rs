use board_core::{
    BoardObject, Color, Document, Error, History, MAX_MATH_DEPTH, MathLayout, ObjectKind,
    Operation, Point, new_id,
};

fn text(id: &str) -> Operation {
    Operation::Add {
        object: BoardObject {
            id: id.into(),
            kind: ObjectKind::Text {
                position: Point::default(),
                text: id.into(),
                size: 24.0,
                color: Color::default(),
            },
        },
    }
}

#[test]
fn failed_transaction_preserves_document_saved_point_and_redo_contents() {
    let mut document = Document::new();
    let page = document.current_page().id.clone();
    let mut history = History::new(&document);
    history
        .apply(&mut document, &page, 0, &[text("saved")])
        .unwrap();
    history.mark_saved(&document);
    history
        .apply(&mut document, &page, 1, &[text("future")])
        .unwrap();
    let future = document.pages.clone();
    history.undo(&mut document).unwrap();
    let before = document.clone();
    let revision = document.revision;
    assert!(matches!(
        history.apply(
            &mut document,
            &page,
            revision,
            &[text("temporary"), text("saved")]
        ),
        Err(Error::DuplicateId(_))
    ));
    assert_eq!(document, before);
    assert!(!history.is_dirty(&document));
    assert!(history.can_undo());
    assert!(history.can_redo());
    history.redo(&mut document).unwrap();
    assert_eq!(document.pages, future);
    assert_eq!(document.revision, revision + 1);
}

#[test]
fn deepest_valid_row_fraction_layout_round_trips() {
    let mut layout = MathLayout::Text("x".into());
    for depth in 1..MAX_MATH_DEPTH {
        layout = if depth % 2 == 0 {
            MathLayout::Row(vec![layout])
        } else {
            MathLayout::Fraction(Box::new(layout), Box::new(MathLayout::Text("1".into())))
        };
    }
    let mut document = Document::new();
    document.pages[0].objects.push(BoardObject {
        id: "deep-math".into(),
        kind: ObjectKind::Math {
            position: Point::default(),
            layout,
            size: 24.0,
            color: Color::default(),
        },
    });
    document.validate().unwrap();
    assert_eq!(
        Document::from_json(&document.to_json().unwrap()).unwrap(),
        document
    );
}

struct TestDirectory(std::path::PathBuf);
impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn invalid_save_preserves_target_dirty_and_history() {
    let directory =
        TestDirectory(std::env::temp_dir().join(format!("board-core-release-{}", new_id())));
    std::fs::create_dir(&directory.0).unwrap();
    let path = directory.0.join("saved.neoboard");
    let mut document = Document::new();
    let page = document.current_page().id.clone();
    let mut history = History::new(&document);
    history.save(&document, &path).unwrap();
    let saved = std::fs::read(&path).unwrap();
    history
        .apply(&mut document, &page, 0, &[text("unsaved")])
        .unwrap();
    let before = document.clone();
    let duplicate = document.pages[0].objects[0].clone();
    document.pages[0].objects.push(duplicate);
    assert!(history.save(&document, &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), saved);
    assert_eq!(std::fs::read_dir(&directory.0).unwrap().count(), 1);
    assert!(history.is_dirty(&document));
    document = before;
    assert!(history.is_dirty(&document));
    assert!(history.undo(&mut document).unwrap());
    assert!(!history.is_dirty(&document));
}
