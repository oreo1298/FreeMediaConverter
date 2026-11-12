// SPDX-License-Identifier: GPL-3.0-or-later
//! Two small layout widgets the libadwaita 1.5 baseline lacks:
//! - `WrapBox`: children flow left to right and wrap onto new lines
//!   (AdwWrapBox needs libadwaita 1.7).
//! - `FixedBin`: one child at an exact size, whatever its natural size
//!   (keeps thumbnails from growing when a card gets taller).

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

mod imp {
    use std::cell::Cell;

    use super::*;

    #[derive(Default)]
    pub struct WrapBox {
        pub spacing: Cell<i32>,
        pub line_spacing: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WrapBox {
        const NAME: &'static str = "RetortWrapBox";
        type Type = super::WrapBox;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for WrapBox {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for WrapBox {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let children = self.children();
            if orientation == gtk::Orientation::Horizontal {
                let mut min = 0;
                let mut nat = 0;
                for (i, c) in children.iter().enumerate() {
                    let (cmin, cnat, _, _) = c.measure(gtk::Orientation::Horizontal, -1);
                    min = min.max(cmin);
                    nat += cnat + if i > 0 { self.spacing.get() } else { 0 };
                }
                (min, nat, -1, -1)
            } else {
                let width = if for_size < 0 { i32::MAX } else { for_size };
                let lines = self.layout(&children, width);
                let h = lines.iter().map(|l| l.height).sum::<i32>()
                    + self.line_spacing.get() * (lines.len().saturating_sub(1) as i32);
                (h, h, -1, -1)
            }
        }

        fn size_allocate(&self, width: i32, _height: i32, _baseline: i32) {
            let children = self.children();
            let mut y = 0;
            for line in self.layout(&children, width) {
                let mut x = 0;
                for (child, w, h) in &line.items {
                    let cy = y + (line.height - h) / 2;
                    child.size_allocate(&gtk::Allocation::new(x, cy, *w, *h), -1);
                    x += w + self.spacing.get();
                }
                y += line.height + self.line_spacing.get();
            }
        }
    }

    pub struct Line {
        pub items: Vec<(gtk::Widget, i32, i32)>,
        pub height: i32,
    }

    impl WrapBox {
        fn children(&self) -> Vec<gtk::Widget> {
            let mut v = Vec::new();
            let mut c = self.obj().first_child();
            while let Some(w) = c {
                c = w.next_sibling();
                if w.should_layout() {
                    v.push(w);
                }
            }
            v
        }

        /// Greedy line breaking with natural widths, clamped to `width`.
        fn layout(&self, children: &[gtk::Widget], width: i32) -> Vec<Line> {
            let spacing = self.spacing.get();
            let mut lines: Vec<Line> = Vec::new();
            let mut x = 0;
            for c in children {
                let (cmin, cnat, _, _) = c.measure(gtk::Orientation::Horizontal, -1);
                let w = cnat.min(width).max(cmin);
                let (_, h, _, _) = c.measure(gtk::Orientation::Vertical, w);
                let start_new = match lines.last() {
                    None => true,
                    Some(l) => !l.items.is_empty() && x + spacing + w > width,
                };
                if start_new {
                    lines.push(Line {
                        items: Vec::new(),
                        height: 0,
                    });
                    x = 0;
                } else {
                    x += spacing;
                }
                let line = lines.last_mut().expect("line");
                line.items.push((c.clone(), w, h));
                line.height = line.height.max(h);
                x += w;
            }
            lines
        }
    }
}

mod fixed {
    use std::cell::Cell;

    use super::*;

    #[derive(Default)]
    pub struct FixedBin {
        pub width: Cell<i32>,
        pub height: Cell<i32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for FixedBin {
        const NAME: &'static str = "RetortFixedBin";
        type Type = super::FixedBin;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for FixedBin {
        fn dispose(&self) {
            while let Some(child) = self.obj().first_child() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for FixedBin {
        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let s = if orientation == gtk::Orientation::Horizontal {
                self.width.get()
            } else {
                self.height.get()
            };
            (s, s, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, _baseline: i32) {
            if let Some(c) = self.obj().first_child() {
                // Children must be measured before allocation.
                let _ = c.measure(gtk::Orientation::Horizontal, -1);
                let _ = c.measure(gtk::Orientation::Vertical, width);
                c.size_allocate(&gtk::Allocation::new(0, 0, width, height), -1);
            }
        }
    }
}

glib::wrapper! {
    pub struct FixedBin(ObjectSubclass<fixed::FixedBin>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl FixedBin {
    pub fn new(child: &impl IsA<gtk::Widget>, width: i32, height: i32) -> Self {
        let b: Self = glib::Object::new();
        child.set_parent(&b);
        b.set_size(width, height);
        b
    }

    pub fn set_size(&self, width: i32, height: i32) {
        self.imp().width.set(width);
        self.imp().height.set(height);
        self.queue_resize();
    }
}

glib::wrapper! {
    pub struct WrapBox(ObjectSubclass<imp::WrapBox>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl WrapBox {
    pub fn new(spacing: i32, line_spacing: i32) -> Self {
        let b: Self = glib::Object::new();
        b.imp().spacing.set(spacing);
        b.imp().line_spacing.set(line_spacing);
        b
    }

    pub fn append(&self, child: &impl IsA<gtk::Widget>) {
        child.set_parent(self);
        self.queue_resize();
    }

    pub fn remove_all(&self) {
        while let Some(c) = self.first_child() {
            c.unparent();
        }
        self.queue_resize();
    }
}
