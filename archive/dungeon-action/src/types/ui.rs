//! UI page / dialog types.

use bevy_ecs::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DialogKind {
    Quit,
    Descend,
}

impl DialogKind {
    pub fn title(self) -> &'static str {
        match self {
            DialogKind::Quit => "确认退出？",
            DialogKind::Descend => "确认下楼？",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    Game,
    Look,
    ThrowSelect,
    ThrowAim,
    Inventory,
    Dialog(DialogKind),
}

#[derive(Resource, Default)]
pub struct PageStack(pub Vec<Page>);

impl PageStack {
    pub fn push(&mut self, page: Page) {
        self.0.push(page);
    }
    pub fn pop(&mut self) -> Option<Page> {
        self.0.pop()
    }
    pub fn current(&self) -> &Page {
        self.0.last().unwrap_or(&Page::Game)
    }
}
