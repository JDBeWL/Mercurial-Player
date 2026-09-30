//! Mercurial Player - 主入口（二进制壳）
//!
//! 启动逻辑全在 [`mercurial_player_lib::run`]，移动端入口（`mobile_entry_point`）才能复用同一套初始化。
//!
//! Copyright (C) 2026  JDBeWL
//!
//! This program is free software: you can redistribute it and/or modify
//! it under the terms of the GNU General Public License as published by
//! the Free Software Foundation, either version 3 of the License, or
//! (at your option) any later version.
//!
//! This program is distributed in the hope that it will be useful,
//! but WITHOUT ANY WARRANTY; without even the implied warranty of
//! MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//! GNU General Public License for more details.
//!
//! You should have received a copy of the GNU General Public License
//! along with this program.  If not, see <https://www.gnu.org/licenses/>.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    mercurial_player_lib::run();
}
