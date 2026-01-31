use project::Vfs;

#[test]
fn test_crash_recovery_logic() {
    let path = "test_crash.vfs";
    let _ = std::fs::remove_file(path);

    {
        let mut vfs = Vfs::create(path, 1024 * 1024).unwrap();
        vfs.create_file("/incert.txt").unwrap();
        let mut inode = vfs.get_inode(1).unwrap();
        inode.is_valid = 0;
        vfs.save_inode(1, inode).unwrap();
    }
    let mut vfs_recovered = Vfs::open(path).expect("Eroare la redeschidere");
    let entries = vfs_recovered.read_dir("/").unwrap();
    println!("Entries după recovery: {:?}", entries);
    let result = vfs_recovered.open_file("/incert.txt");

    if result.is_ok() {
        let inode = vfs_recovered.get_inode(1);
        match inode {
            Ok(i) => {
                println!("Inode găsit: is_valid={}, size={}", i.is_valid, i.size);
                panic!("Fișierul corupt nu a fost eliminat complet!");
            }
            Err(_) => {
                println!(" Inode-ul a fost dealocat corect");
            }
        }
    } else {
        println!(" Fișierul nu poate fi deschis (inode invalid)");
    }

    std::fs::remove_file(path).ok();
}
