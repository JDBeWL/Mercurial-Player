# GEMINI.md

## Project Overview

This project is a cross-platform music player built with modern web technologies. It uses a combination of a web-based frontend and a native backend to provide a feature-rich and performant user experience.

- **Frontend:** The frontend is a single-page application (SPA) built with [Vue.js](https://vuejs.org/) and [Vite](https://vitejs.dev/). It utilizes [Pinia](https://pinia.vuejs.org/) for state management and [Vue I18n](https://vue-i18n.intlify.dev/) for internationalization, supporting both English and Chinese languages. The user interface is crafted with a modern design, featuring theme selection, dynamic layouts, and smooth transitions.

- **Backend:** The backend is developed in [Rust](https://www.rust-lang.org/) and is powered by the [Tauri](https://tauri.app/) framework. This allows the application to run as a native desktop application with access to system-level resources. The backend is responsible for core functionalities such as:
  - **Audio Playback:** Managing audio playback, including playing, pausing, seeking, and volume control.
  - **File System:** Accessing the local file system to read music files, directories, and lyrics.
  - **Metadata:** Extracting track metadata, such as title, artist, and album art.
  - **Configuration:** Handling application settings and user preferences.

- **Architecture:** The application follows a modern frontend architecture, with a clear separation of concerns between the UI components, state management, and utility modules. The frontend communicates with the Rust backend through a secure and efficient IPC (Inter-Process Communication) bridge provided by Tauri. This allows the frontend to invoke backend functions and receive data in a seamless and asynchronous manner.

## Building and Running

### Prerequisites

- [Node.js](https://nodejs.org/) (with `npm` or `pnpm`)
- [Rust](https://www.rust-lang.org/)

### Development

To run the application in a development environment with hot-reloading, use the following command:

```bash
npm run tauri:dev
```

This will start a local development server for the frontend and launch the Tauri application in a new window.

### Production Build

To build the application for production, which includes optimizations and bundling for distribution, run the following command:

```bash
npm run tauri:build
```

This will generate a native executable for your platform in the `src-tauri/target/release` directory.

## Development Conventions

- **State Management:** The application uses Pinia for centralized state management. The `src/stores` directory contains modules for managing different parts of the application state, such as the player, theme, and configuration.

- **Internationalization:** Text strings are managed in JSON files located in the `src/locales` directory. The `vue-i18n` library is used to provide multi-language support.

- **Backend Commands:** The backend exposes a set of commands that can be invoked from the frontend. These commands are defined in the `src-tauri/src/main.rs` file and are organized into different modules based on their functionality.

- **Styling:** The application uses a combination of global styles and scoped styles for its components. The main stylesheet is located at `src/style.css`, and component-specific styles are defined within the `.vue` files.

- **File Structure:** The project is organized into two main directories:
  - `src`: Contains the frontend source code, including Vue components, stores, and utilities.
  - `src-tauri`: Contains the backend Rust source code, Tauri configuration, and application icons.
