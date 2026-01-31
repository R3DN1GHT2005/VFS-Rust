use chrono::{DateTime, Utc};
use std::cell::RefCell;
use std::fs::{File, OpenOptions};
use std::io::{self, Error, Read, Seek, SeekFrom, Write};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

pub mod models;
use models::{BLOCK_SIZE, DirEntry, INODE_SIZE, Inode, KEY, MAX_NAME_LEN, SuperBlock};

pub mod file;
pub use file::VfsFile;

use crate::models::DIR_SIZE;

pub struct Vfs {
    file: Rc<RefCell<File>>,
    sb: SuperBlock,
}

impl Vfs {
    pub fn create(path: &str, total_size: u64) -> io::Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        file.set_len(total_size)?;

        let total_blocks = (total_size / BLOCK_SIZE as u64) as u32;
        let max_inodes = total_blocks / 4;

        let sb_size = BLOCK_SIZE as u64;
        let inode_bitmap_size = ((max_inodes as f32 / 8.0).ceil() as u64).max(1);
        let data_bitmap_size = ((total_blocks as f32 / 8.0).ceil() as u64).max(1);
        let inode_table_size = max_inodes as u64 * std::mem::size_of::<Inode>() as u64;

        let inode_bitmap_st = sb_size;
        let data_bitmap_st = inode_bitmap_st + inode_bitmap_size;
        let inode_table_st = data_bitmap_st + data_bitmap_size;

        let data_blocks_st = ((inode_table_st + inode_table_size + BLOCK_SIZE as u64 - 1)
            .div_ceil(BLOCK_SIZE as u64))
            * BLOCK_SIZE as u64;

        let sb = SuperBlock {
            key: KEY,
            block_size: BLOCK_SIZE as u32,
            total_blocks,
            inode_bitmap_start: inode_bitmap_st,
            data_bitmap_start: data_bitmap_st,
            inode_table_start: inode_table_st,
            data_blocks_start: data_blocks_st,
        };

        file.seek(SeekFrom::Start(0))?;
        file.write_all(&sb.to_bytes())?;

        let zero_block = vec![0u8; BLOCK_SIZE];
        let metadata_area_size = data_blocks_st - inode_bitmap_st;
        let mut written = 0;
        file.seek(SeekFrom::Start(inode_bitmap_st))?;
        while written < metadata_area_size {
            let chunk = std::cmp::min(BLOCK_SIZE as u64, metadata_area_size - written);
            file.write_all(&zero_block[..chunk as usize])?;
            written += chunk;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let root_inode = Inode {
            inode_type: 1,
            is_valid: 1,
            size: 0,
            created_at: now,
            modified_at: now,
            direct_blocks: [0; 10],
            indirect_blocks: 0,
        };

        file.seek(SeekFrom::Start(inode_table_st))?;
        file.write_all(&root_inode.to_bytes())?;

        file.seek(SeekFrom::Start(inode_bitmap_st))?;
        file.write_all(&[0b00000001])?;

        file.sync_all()?;
        let mut vfs = Vfs {
            file: Rc::new(RefCell::new(file)),
            sb,
        };

        vfs.add_entry_to_parent(0, ".", 0)?;
        vfs.add_entry_to_parent(0, "..", 0)?;

        Ok(vfs)
    }

    pub fn open(name: &str) -> io::Result<Self> {
        let mut file = OpenOptions::new().read(true).write(true).open(name)?;
        let mut buffer = vec![0u8; std::mem::size_of::<SuperBlock>()];
        file.seek(SeekFrom::Start(0))?;
        file.read_exact(&mut buffer)?;

        let sb = SuperBlock::from_bytes(&buffer);
        if sb.key != KEY {
            return Err(Error::new(
                io::ErrorKind::InvalidData,
                "Not supported by library!",
            ));
        }

        let mut vfs = Vfs {
            file: Rc::new(RefCell::new(file)),
            sb,
        };

        vfs.recover_corrupted_inodes()?;

        Ok(vfs)
    }

    fn recover_corrupted_inodes(&mut self) -> io::Result<()> {
        let max_inodes = (self.sb.data_bitmap_start - self.sb.inode_bitmap_start) * 8;
        let mut recovered_count = 0;

        for inode_id in 1..max_inodes as u32 {
            if !self.is_inode_allocated(inode_id)? {
                continue;
            }

            let inode = self.get_inode(inode_id)?;
            if inode.is_valid == 0 {
                self.deallocate_inode(inode_id)?;
                recovered_count += 1;
            }
        }

        if recovered_count > 0 {
            println!("{} corrupted inodes!", recovered_count);
        }

        Ok(())
    }

    fn is_inode_allocated(&mut self, inode_id: u32) -> io::Result<bool> {
        let byte_offset = inode_id / 8;
        let bit_offset = inode_id % 8;

        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(
            self.sb.inode_bitmap_start + byte_offset as u64,
        ))?;
        let mut byte = [0u8; 1];
        file.read_exact(&mut byte)?;

        Ok((byte[0] & (1 << bit_offset)) != 0)
    }

    fn deallocate_inode(&mut self, inode_id: u32) -> io::Result<()> {
        let byte_offset = inode_id / 8;
        let bit_offset = inode_id % 8;

        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(
            self.sb.inode_bitmap_start + byte_offset as u64,
        ))?;
        let mut byte = [0u8; 1];
        file.read_exact(&mut byte)?;

        byte[0] &= !(1 << bit_offset);
        file.seek(SeekFrom::Start(
            self.sb.inode_bitmap_start + byte_offset as u64,
        ))?;
        file.write_all(&byte)?;

        Ok(())
    }

    fn allocate_bit(&mut self, start: u64, end: u64) -> io::Result<u32> {
        let total_bytes = end - start;
        let mut buffer = [0u8; 512];
        let mut file = self.file.borrow_mut();

        for chunk_idx in 0..(total_bytes / 512 + 1) {
            let current_offset = start + (chunk_idx * 512);
            let to_read = std::cmp::min(512, total_bytes - (chunk_idx * 512));
            if to_read == 0 {
                break;
            }

            file.seek(SeekFrom::Start(current_offset))?;
            file.read_exact(&mut buffer[..to_read as usize])?;

            for (byte_idx, byte) in buffer[..to_read as usize].iter_mut().enumerate() {
                if *byte != 0xFF {
                    for bit_idx in 0..8 {
                        if (*byte & (1 << bit_idx)) == 0 {
                            *byte |= 1 << bit_idx;
                            file.seek(SeekFrom::Start(current_offset + byte_idx as u64))?;
                            file.write_all(&[*byte])?;
                            return Ok((chunk_idx as u32 * 512 * 8)
                                + (byte_idx as u32 * 8)
                                + bit_idx as u32);
                        }
                    }
                }
            }
        }
        Err(Error::other("No more free inodes!"))
    }

    fn allocate_inode(&mut self) -> io::Result<u32> {
        self.allocate_bit(self.sb.inode_bitmap_start, self.sb.data_bitmap_start)
    }

    pub(crate) fn allocate_data_block(&mut self) -> io::Result<u32> {
        self.allocate_bit(self.sb.data_bitmap_start, self.sb.inode_table_start)
    }

    pub fn get_inode(&mut self, id: u32) -> io::Result<Inode> {
        let pos = self.sb.inode_table_start + (id as u64 * INODE_SIZE as u64);
        let mut buffer = [0u8; INODE_SIZE];
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(pos))?;
        file.read_exact(&mut buffer)?;
        Ok(Inode::from_bytes(&buffer))
    }

    pub fn save_inode(&mut self, id: u32, inode: Inode) -> io::Result<()> {
        let pos = self.sb.inode_table_start + (id as u64 * INODE_SIZE as u64);
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(pos))?;
        file.write_all(&inode.to_bytes())?;
        Ok(())
    }

    pub fn find_inode_by_path(&mut self, path: &str) -> io::Result<u32> {
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let mut current_id = 0;
        for part in parts {
            current_id = self.find_in_dir(current_id, part)?;
        }
        Ok(current_id)
    }

    fn find_in_dir(&mut self, dir_id: u32, name: &str) -> io::Result<u32> {
        let dir_inode = self.get_inode(dir_id)?;

        for block_index in 0..1034 {
            let physical_id = match self.just_read(&dir_inode, block_index)? {
                Some(id) => id,
                None => break,
            };

            let block_pos = self.sb.data_blocks_start + (physical_id as u64 * BLOCK_SIZE as u64);

            for i in 0..(BLOCK_SIZE / DIR_SIZE) {
                let mut file = self.file.borrow_mut();
                file.seek(SeekFrom::Start(block_pos + (i as u64 * DIR_SIZE as u64)))?;
                let mut buffer = [0u8; DIR_SIZE];
                file.read_exact(&mut buffer)?;
                drop(file);

                let entry = DirEntry::from_bytes(&buffer);

                if entry.is_active == 1 {
                    let entry_name = std::str::from_utf8(&entry.name)
                        .unwrap_or("")
                        .trim_matches('\0');
                    if entry_name == name {
                        if !self.is_inode_allocated(entry.inode_id)? {
                            return Err(Error::new(
                                io::ErrorKind::NotFound,
                                format!("Inode for '{}' is corrupted!", name),
                            ));
                        }
                        return Ok(entry.inode_id);
                    }
                }
            }
        }
        Err(Error::new(
            io::ErrorKind::NotFound,
            format!("Name '{}' does not exist!", name),
        ))
    }

    pub fn create_dir(&mut self, path: &str) -> io::Result<()> {
        let (parent_path, new_name) = path
            .rfind('/')
            .map_or(("", path), |pos| (&path[..pos], &path[pos + 1..]));

        let parent_id = if parent_path.is_empty() {
            0
        } else {
            self.find_inode_by_path(parent_path)?
        };

        let new_id = self.allocate_inode()?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let inode = Inode {
            inode_type: 1,
            is_valid: 1,
            size: 0,
            created_at: now,
            modified_at: now,
            direct_blocks: [0; 10],
            indirect_blocks: 0,
        };

        self.save_inode(new_id, inode)?;
        self.add_entry_to_parent(parent_id, new_name, new_id)?;
        self.add_entry_to_parent(new_id, ".", new_id)?;
        self.add_entry_to_parent(new_id, "..", parent_id)?;

        Ok(())
    }

    fn add_entry_to_parent(&mut self, parent_id: u32, name: &str, child_id: u32) -> io::Result<()> {
        let mut name_bytes = [0u8; MAX_NAME_LEN];
        let bytes = name.as_bytes();
        let len = std::cmp::min(bytes.len(), MAX_NAME_LEN);
        name_bytes[..len].copy_from_slice(&bytes[..len]);

        let entry = DirEntry {
            inode_id: child_id,
            name: name_bytes,
            is_active: 1,
        };

        let pointers_per_block = (BLOCK_SIZE / 4) as u32;
        let max_blocks = 10 + pointers_per_block;

        for block_index in 0..max_blocks {
            let physical_id = self.allocate_indirect_or_direct_blocks(parent_id, block_index)?;
            let block_pos = self.sb.data_blocks_start + (physical_id as u64 * BLOCK_SIZE as u64);
            for i in 0..(BLOCK_SIZE / DIR_SIZE) {
                let entry_pos = block_pos + (i as u64 * DIR_SIZE as u64);

                let mut file = self.file.borrow_mut();
                file.seek(SeekFrom::Start(entry_pos))?;
                let mut buf = [0u8; DIR_SIZE];
                file.read_exact(&mut buf)?;
                if DirEntry::from_bytes(&buf).is_active == 0 {
                    file.seek(SeekFrom::Start(entry_pos))?;
                    file.write_all(&entry.to_bytes())?;
                    drop(file);

                    let mut parent_inode = self.get_inode(parent_id)?;
                    let now = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|e| Error::other(e.to_string()))?
                        .as_secs();
                    parent_inode.modified_at = now;
                    let entry_end_pos = (block_index as u64 * BLOCK_SIZE as u64)
                        + ((i + 1) as u64 * DIR_SIZE as u64);
                    if entry_end_pos > parent_inode.size {
                        parent_inode.size = entry_end_pos;
                    }

                    self.save_inode(parent_id, parent_inode)?;

                    return Ok(());
                }
            }
        }

        Err(Error::other("Directory is full or size limit reached!"))
    }

    pub fn create_file(&mut self, path: &str) -> io::Result<VfsFile> {
        let (parent_path, file_name) = path
            .rfind('/')
            .map_or(("", path), |pos| (&path[..pos], &path[pos + 1..]));
        let parent_id = if parent_path.is_empty() {
            0
        } else {
            self.find_inode_by_path(parent_path)?
        };

        let new_id = self.allocate_inode()?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| Error::other(e.to_string()))?
            .as_secs();
        let inode = Inode {
            inode_type: 0,
            is_valid: 1,
            size: 0,
            created_at: now,
            modified_at: now,
            direct_blocks: [0; 10],
            indirect_blocks: 0,
        };

        self.save_inode(new_id, inode)?;
        self.add_entry_to_parent(parent_id, file_name, new_id)?;
        self.file.borrow_mut().sync_all()?;

        Ok(VfsFile {
            file: Rc::clone(&self.file),
            sb: self.sb,
            inode_id: new_id,
            position: 0,
        })
    }

    pub fn open_file(&mut self, path: &str) -> io::Result<VfsFile> {
        let inode_id = self.find_inode_by_path(path)?;
        Ok(VfsFile {
            file: Rc::clone(&self.file),
            sb: self.sb,
            inode_id,
            position: 0,
        })
    }

    pub fn read_dir(&mut self, path: &str) -> io::Result<Vec<String>> {
        let dir_id = self.find_inode_by_path(path)?;
        let dir_inode = self.get_inode(dir_id)?;

        if dir_inode.inode_type != 1 {
            return Err(Error::other("Not a directory!"));
        }

        let mut entries = Vec::new();
        for block_index in 0..1034 {
            let physical_id = match self.just_read(&dir_inode, block_index)? {
                Some(id) => id,
                None => break,
            };

            let block_pos = self.sb.data_blocks_start + (physical_id as u64 * BLOCK_SIZE as u64);
            for i in 0..(BLOCK_SIZE / DIR_SIZE) {
                let mut file = self.file.borrow_mut();
                file.seek(SeekFrom::Start(block_pos + (i as u64 * DIR_SIZE as u64)))?;
                let mut buf = [0u8; DIR_SIZE];
                file.read_exact(&mut buf)?;
                drop(file);

                let entry = DirEntry::from_bytes(&buf);

                if entry.is_active == 1 {
                    let name = std::str::from_utf8(&entry.name)
                        .unwrap_or("")
                        .trim_matches('\0')
                        .to_string();
                    entries.push(name);
                }
            }
        }
        Ok(entries)
    }

    pub fn allocate_indirect_or_direct_blocks(
        &mut self,
        inode_id: u32,
        block_index: u32,
    ) -> io::Result<u32> {
        let mut inode = self.get_inode(inode_id)?;
        if block_index < 10 {
            let direct_block = inode.direct_blocks[block_index as usize];

            if direct_block == 0 {
                let new_block_id = self.allocate_data_block()?;
                inode.direct_blocks[block_index as usize] = new_block_id;
                self.save_inode(inode_id, inode)?;
                return Ok(new_block_id);
            }
            return Ok(direct_block);
        }
        let indirect_block_index = block_index - 10;
        let max_pointers_per_block = (BLOCK_SIZE / 4) as u32;
        if indirect_block_index >= max_pointers_per_block {
            return Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                format!(
                    "File is too large! Maximum {} blocks supported.",
                    10 + max_pointers_per_block
                ),
            ));
        }
        if inode.indirect_blocks == 0 {
            let new_pointer_block = self.allocate_data_block()?;
            inode.indirect_blocks = new_pointer_block;
            self.save_inode(inode_id, inode)?;
            let buffer = vec![0u8; BLOCK_SIZE];
            let disk_position =
                self.sb.data_blocks_start + (new_pointer_block as u64 * BLOCK_SIZE as u64);
            let mut file = self.file.borrow_mut();
            file.seek(SeekFrom::Start(disk_position))?;
            file.write_all(&buffer)?;
        }

        let indirect_block_disk_start =
            self.sb.data_blocks_start + (inode.indirect_blocks as u64 * BLOCK_SIZE as u64);
        let pointer_address_on_disk = indirect_block_disk_start + (indirect_block_index as u64 * 4);

        let mut pointer_bytes = [0u8; 4];
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(pointer_address_on_disk))?;
        file.read_exact(&mut pointer_bytes)?;

        let mut data_block_pointer = u32::from_le_bytes(pointer_bytes);

        if data_block_pointer == 0 {
            drop(file);
            data_block_pointer = self.allocate_data_block()?;
            let mut file = self.file.borrow_mut();
            file.seek(SeekFrom::Start(pointer_address_on_disk))?;
            file.write_all(&data_block_pointer.to_le_bytes())?;
        }

        Ok(data_block_pointer)
    }

    fn just_read(&mut self, inode: &Inode, block_index: u32) -> io::Result<Option<u32>> {
        if block_index < 10 {
            let id = inode.direct_blocks[block_index as usize];
            return Ok(if id == 0 { None } else { Some(id) });
        }

        if inode.indirect_blocks == 0 {
            return Ok(None);
        }

        let indirect_idx = block_index - 10;
        let pointer_pos = self.sb.data_blocks_start
            + (inode.indirect_blocks as u64 * BLOCK_SIZE as u64)
            + (indirect_idx as u64 * 4);

        let mut buf = [0u8; 4];
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(pointer_pos))?;
        file.read_exact(&mut buf)?;
        let id = u32::from_le_bytes(buf);

        Ok(if id == 0 { None } else { Some(id) })
    }
    pub fn remove(&mut self, path: &str) -> io::Result<()> {
        let (parent_path, name) = path
            .rfind('/')
            .map_or(("", path), |pos| (&path[..pos], &path[pos + 1..]));

        let parent_id = if parent_path.is_empty() {
            0
        } else {
            self.find_inode_by_path(parent_path)?
        };
        let inode_id = self.find_in_dir(parent_id, name)?;
        let inode = self.get_inode(inode_id)?;
        for i in 0..10 {
            if inode.direct_blocks[i] != 0 {
                self.free_bit(self.sb.data_bitmap_start, inode.direct_blocks[i])?;
            }
        }
        if inode.indirect_blocks != 0 {
            let mut pointer_buf = [0u8; BLOCK_SIZE];
            let pos =
                self.sb.data_blocks_start + (inode.indirect_blocks as u64 * BLOCK_SIZE as u64);
            let mut file = self.file.borrow_mut();
            file.seek(SeekFrom::Start(pos))?;
            file.read_exact(&mut pointer_buf)?;
            drop(file);

            for chunk in pointer_buf.chunks_exact(4) {
                let block_ptr = u32::from_le_bytes(chunk.try_into().unwrap());
                if block_ptr != 0 {
                    self.free_bit(self.sb.data_bitmap_start, block_ptr)?;
                }
            }
            self.free_bit(self.sb.data_bitmap_start, inode.indirect_blocks)?;
        }
        self.free_bit(self.sb.inode_bitmap_start, inode_id)?;
        self.set_entry_active_status(parent_id, name, 0)?;

        Ok(())
    }

    fn free_bit(&mut self, start_offset: u64, bit_idx: u32) -> io::Result<()> {
        let byte_pos = (bit_idx / 8) as u64;
        let bit_pos = (bit_idx % 8) as u8;

        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(start_offset + byte_pos))?;
        let mut byte = [0u8; 1];
        file.read_exact(&mut byte)?;

        byte[0] &= !(1 << bit_pos);

        file.seek(SeekFrom::Start(start_offset + byte_pos))?;
        file.write_all(&byte)?;
        Ok(())
    }

    fn set_entry_active_status(&mut self, dir_id: u32, name: &str, status: u8) -> io::Result<()> {
        let dir_inode = self.get_inode(dir_id)?;
        let max_blocks = 10 + (BLOCK_SIZE / 4) as u32;

        for block_index in 0..max_blocks {
            let physical_id = match self.just_read(&dir_inode, block_index)? {
                Some(id) => id,
                None => break,
            };

            let block_pos = self.sb.data_blocks_start + (physical_id as u64 * BLOCK_SIZE as u64);
            for i in 0..(BLOCK_SIZE / DIR_SIZE) {
                let entry_pos = block_pos + (i as u64 * DIR_SIZE as u64);
                let mut file = self.file.borrow_mut();
                file.seek(SeekFrom::Start(entry_pos))?;
                let mut buf = [0u8; DIR_SIZE];
                file.read_exact(&mut buf)?;
                let mut entry = DirEntry::from_bytes(&buf);

                let entry_name = std::str::from_utf8(&entry.name)
                    .unwrap_or("")
                    .trim_matches('\0');
                if entry.is_active == 1 && entry_name == name {
                    entry.is_active = status;
                    file.seek(SeekFrom::Start(entry_pos))?;
                    file.write_all(&entry.to_bytes())?;
                    return Ok(());
                }
            }
        }
        Err(Error::new(io::ErrorKind::NotFound, "Entry not found!"))
    }
    pub fn stat(&mut self, path: &str) -> io::Result<Inode> {
        let inode_id = self.find_inode_by_path(path)?;
        self.get_inode(inode_id)
    }
    pub fn list_long(&mut self, path: &str) -> io::Result<()> {
        let dir_id = self.find_inode_by_path(path)?;
        let dir_inode = self.get_inode(dir_id)?;

        if dir_inode.inode_type != 1 {
            return Err(Error::other("Not a directory!"));
        }

        println!(
            "{:<6} {:<10} {:<20} {:<20} {:<}",
            "Type", "Size", "Created At", "Modified At", "Name"
        );
        println!("{}", "-".repeat(90));

        let max_blocks = 10 + (BLOCK_SIZE / 4) as u32;
        for block_index in 0..max_blocks {
            let physical_id = match self.just_read(&dir_inode, block_index)? {
                Some(id) => id,
                None => break,
            };

            let block_pos = self.sb.data_blocks_start + (physical_id as u64 * BLOCK_SIZE as u64);
            for i in 0..(BLOCK_SIZE / DIR_SIZE) {
                let mut file = self.file.borrow_mut();
                file.seek(SeekFrom::Start(block_pos + (i as u64 * DIR_SIZE as u64)))?;
                let mut buf = [0u8; DIR_SIZE];
                file.read_exact(&mut buf)?;
                let entry = DirEntry::from_bytes(&buf);
                drop(file);

                if entry.is_active == 1 {
                    let inode = self.get_inode(entry.inode_id)?;

                    let created_at = DateTime::from_timestamp(inode.created_at as i64, 0)
                        .unwrap_or_default()
                        .with_timezone(&Utc)
                        .format("%Y-%m-%d %H:%M:%S");

                    let modified_at = DateTime::from_timestamp(inode.modified_at as i64, 0)
                        .unwrap_or_default()
                        .with_timezone(&Utc)
                        .format("%Y-%m-%d %H:%M:%S");

                    let type_str = if inode.inode_type == 1 { "DIR" } else { "FILE" };
                    let name = std::str::from_utf8(&entry.name)
                        .unwrap_or("")
                        .trim_matches('\0');

                    println!(
                        "{:<6} {:<10} {:<20} {:<20} {:<}",
                        type_str, inode.size, created_at, modified_at, name
                    );
                }
            }
        }
        Ok(())
    }
}
