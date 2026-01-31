A virtual file system implementation in Rust that simulates a Unix-like file system with support for files, directories, and advanced features like indirect block addressing for large files.

*Features*:
- **Full File System Operations**: Create, read, write, and delete files and directories
- **Hierarchical Directory Structure**: Support for nested directories with Unix-like paths
- **Large File Support**: Implements both direct and indirect block addressing for files up to 60KB+
- **Metadata Management**: Tracks creation and modification timestamps for all files
- **Persistent Storage**: All data is stored in a single binary file that can be mounted and unmounted
- **Memory Efficient**: Uses bitmap-based allocation for both inodes and data blocks
- **Thread Safety**: Designed with Rust's ownership principles for safe concurrent access

- **Comprehensive Testing**: Includes test suites for:
  - Concurrent operations
  - Crash recovery
  - Large file handling
  - Basic file operations
  - Data persistency

*Arhitecture*:
- **SuperBlock**: Stores file system metadata and layout information
- **Inode Table**: Manages file and directory metadata
- **Data Blocks**: 4KB blocks for storing actual file content
- **Bitmap Allocation**: Efficient tracking of free inodes and data blocks
- **Direct & Indirect Blocks**: Supports files of varying sizes efficiently

This project is open source and available under the [MIT License](LICENSE).
