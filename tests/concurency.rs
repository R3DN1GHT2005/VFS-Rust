use project::Vfs;
use std::io::{Read, Write};

#[test]
fn test_multiple_simultaneous_files() {
    let path = "test_concurrent.vfs";
    let _ = std::fs::remove_file(path);

    let mut vfs = Vfs::create(path, 1024 * 1024).unwrap();

    vfs.create_file("/f1.txt").unwrap();
    vfs.create_file("/f2.txt").unwrap();

    let mut f1 = vfs.open_file("/f1.txt").unwrap();
    let mut f2 = vfs.open_file("/f2.txt").unwrap();

    f1.write_all(b"Fisierul UNU").unwrap();
    f2.write_all(b"Fisierul DOI").unwrap();

    let mut b1 = String::new();
    let mut b2 = String::new();

    use std::io::Seek;
    f1.seek(std::io::SeekFrom::Start(0)).unwrap();
    f2.seek(std::io::SeekFrom::Start(0)).unwrap();

    f1.read_to_string(&mut b1).unwrap();
    f2.read_to_string(&mut b2).unwrap();

    assert_eq!(b1, "Fisierul UNU");
    assert_eq!(b2, "Fisierul DOI");

    std::fs::remove_file(path).ok();
}
