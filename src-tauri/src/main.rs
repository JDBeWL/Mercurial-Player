//! Mercurial Player - 主入口（二进制壳）
//!
//! 全部启动逻辑位于库的 [`mercurial_player::run`]，
//! 这样 Tauri 的 Android/iOS 移动端入口
//! （[`tauri::mobile_entry_point`] 生成的 JNI `Rust.create()`）能复用同一套初始化。
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
    mercurial_player::run();
}
