# Windows installer template

The template is derived from [Tauri CLI 2.11.4](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.4/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi), under the accompanying MIT license. The upstream source SHA-256 is `20f4ecc730defb71f1342eaeaec4021df13be3d843abba0effe88ea5835fa079`. Keep the npm CLI pin and template aligned.

Local changes enforce native x64 and Task-only admission before product writes, fix the program directory under LocalAppData/Programs, replace directory selection with login/tray choices, preserve the headless service during shell upgrades, and delegate optional data removal to the Rust Agent-restore/journal transaction. The uninstaller deletes its inventoried files and empty directories; it never recursively removes an application/state directory.
