use project::Vfs;
use std::io::{Read, Seek, SeekFrom, Write};

fn main() -> std::io::Result<()> {
    let disk_path = "virtual_disk.bin";
    let disk_size = 10 * 1024 * 1024;

    println!("--- 1. Creare Sistem de Fișiere ---");
    let mut vfs = Vfs::create(disk_path, disk_size)?;
    println!("Discul virtual a fost creat: {} octeți\n", disk_size);

    println!("--- 2. Testare Directoare ---");
    vfs.create_dir("/documente")?;
    vfs.create_dir("/documente/poze")?;
    vfs.create_dir("/muzica")?;

    println!("\n=== Conținut Root (/) ===");
    vfs.list_long("/")?;

    let entries = vfs.read_dir("/documente")?;
    println!("\nConținut /documente: {:?}\n", entries);

    println!("--- 3. Testare Scriere Fișier (Blocuri Directe) ---");
    {
        let mut file = vfs.create_file("/documente/note.txt")?;
        file.write_all(b"Salut! Acesta este un test simplu.")?;
        println!("Fișierul 'note.txt' a fost scris.");
    }

    println!("\n--- 4. Testare Scriere Fișier MARE (Blocuri Indirecte) ---");
    {
        let mut big_file = vfs.create_file("/documente/mare.dat")?;
        let data = vec![65u8; 60000]; // 60KB de date
        big_file.write_all(&data)?;
        println!("Fișier mare (60KB) creat. A folosit blocuri indirecte.");
    }

    println!("\n=== Conținut /documente după creare fișiere ===");
    vfs.list_long("/documente")?;

    println!("\n--- 5. Testare Citire și Seek ---");
    {
        let mut file = vfs.open_file("/documente/note.txt")?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        println!("Conținut citit: '{}'", buf);
        file.seek(SeekFrom::Start(7))?;
        let mut word = [0u8; 6];
        file.read_exact(&mut word)?;
        println!(
            "Cuvânt la poziția 7: '{}'",
            std::str::from_utf8(&word).unwrap()
        );
    }

    println!("\n--- 6. Testare Metadate (Stat) ---");
    let info = vfs.stat("/documente/mare.dat")?;
    println!("Mărime fișier mare: {} octeți", info.size);
    println!("Creat la timestamp: {}", info.created_at);

    println!("\n--- 7. Testare Ștergere (Remove) ---");
    vfs.remove("/documente/note.txt")?;
    println!("Fișierul 'note.txt' a fost șters.");

    match vfs.open_file("/documente/note.txt") {
        Err(e) => println!("✓ Confirmare: Fișierul nu mai poate fi deschis ({})", e),
        Ok(_) => println!("✗ EROARE: Fișierul încă există!"),
    }

    println!("\n=== Conținut final /documente ===");
    vfs.list_long("/documente")?;
    println!("\n🎉 --- Test Finalizat cu Succes! ---");
    Ok(())
}
