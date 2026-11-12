// SPDX-License-Identifier: GPL-3.0-or-later
//! Advanced mode (filled in by milestone M3).

use crate::window::Ui;

pub struct Advanced {
    pub root: gtk::Widget,
}

impl Advanced {
    pub fn build() -> Advanced {
        let page = adw::StatusPage::builder()
            .icon_name("preferences-other-symbolic")
            .build();
        Advanced {
            root: glib::object::Cast::upcast(page),
        }
    }
}

pub fn wire(_ui: &Ui) {}

pub fn sync(_ui: &Ui) {}

use gtk::glib;
