use project::Vfs;
use std::io::{Read, Write};

#[test]
fn test_hierarchy_and_simple_io() {
    let path = "test_basic.vfs";
    let _ = std::fs::remove_file(path);

    let mut vfs = Vfs::create(path, 2 * 1024 * 1024).expect("Eroare la creare VFS");
    vfs.create_dir("/home").unwrap();
    vfs.create_dir("/home/user").unwrap();
    vfs.create_dir("/home/user/docs").unwrap();

    {
        let mut f = vfs.create_file("/home/user/docs/hello.txt").unwrap();
        f.write_all(b"Salut Rust!").unwrap();
    }

    let mut f2 = vfs.open_file("/home/user/docs/hello.txt").unwrap();
    let mut buf = String::new();
    f2.read_to_string(&mut buf).unwrap();
    assert_eq!(buf, "Salut Rust!");

    let entries = vfs.read_dir("/home/user/docs").unwrap();
    assert!(entries.contains(&"hello.txt".to_string()));

    std::fs::remove_file(path).ok();
}
