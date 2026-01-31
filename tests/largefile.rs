use project::Vfs;
use std::io::{Read, Seek, SeekFrom, Write};

#[test]
fn test_indirect_blocks_large_file() {
    let path = "test_large.vfs";
    let _ = std::fs::remove_file(path);

    let mut vfs = Vfs::create(path, 5 * 1024 * 1024).unwrap();
    let file_path = "/mare.bin";
    let size = 100 * 1024;
    let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();

    {
        let mut f = vfs.create_file(file_path).unwrap();
        f.write_all(&data).unwrap();
    }

    let mut f_read = vfs.open_file(file_path).unwrap();
    let mut read_buf = Vec::new();
    f_read.read_to_end(&mut read_buf).unwrap();

    assert_eq!(read_buf.len(), size);
    assert_eq!(read_buf, data);
    f_read.seek(SeekFrom::Start(80 * 1024)).unwrap();
    let mut small_buf = [0u8; 4];
    f_read.read_exact(&mut small_buf).unwrap();
    assert_eq!(small_buf, &data[81920..81924]);

    std::fs::remove_file(path).ok();
}
