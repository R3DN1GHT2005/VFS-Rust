use project::Vfs;
use std::io::{Read, Write};
#[test]
fn test_persistence_across_sessions() {
    let path = "test_persistence.vfs";
    let _ = std::fs::remove_file(path);
    let content = b"Aceste date trebuie sa supravietuiasca inchiderii";

    {
        let mut vfs = Vfs::create(path, 1024 * 1024).unwrap();
        vfs.create_dir("/baza_de_date").unwrap();
        let mut f = vfs.create_file("/baza_de_date/config.bin").unwrap();
        f.write_all(content).unwrap();
        f.flush().unwrap();
    }
    {
        let mut vfs_reopened = Vfs::open(path).expect("Eroare la redeschiderea VFS");

        let entries = vfs_reopened.read_dir("/baza_de_date").unwrap();
        assert!(entries.contains(&"config.bin".to_string()));

        let mut f_read = vfs_reopened.open_file("/baza_de_date/config.bin").unwrap();
        let mut buffer = Vec::new();
        f_read.read_to_end(&mut buffer).unwrap();

        assert_eq!(buffer, content);
        println!("Succes: Datele și structura au persistat între sesiuni!");
    }

    let _ = std::fs::remove_file(path).ok();
}
