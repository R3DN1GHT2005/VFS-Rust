use crate::models::{BLOCK_SIZE, INODE_SIZE, Inode, SuperBlock};
use std::cell::RefCell;
use std::fs::File;
use std::io::{self, Error, Read, Seek, SeekFrom, Write};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct VfsFile {
    pub(crate) file: Rc<RefCell<File>>,
    pub(crate) sb: SuperBlock,
    pub inode_id: u32,
    pub position: u64,
}

impl VfsFile {
    fn get_inode(&self) -> io::Result<Inode> {
        let pos = self.sb.inode_table_start + (self.inode_id as u64 * INODE_SIZE as u64);
        let mut buffer = [0u8; INODE_SIZE];
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(pos))?;
        file.read_exact(&mut buffer)?;
        Ok(Inode::from_bytes(&buffer))
    }

    fn save_inode(&self, inode: &Inode) -> io::Result<()> {
        let pos = self.sb.inode_table_start + (self.inode_id as u64 * INODE_SIZE as u64);
        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(pos))?;
        file.write_all(&inode.to_bytes())?;
        Ok(())
    }

    fn allocate_data_block(&self) -> io::Result<u32> {
        let total_bytes = self.sb.inode_table_start - self.sb.data_bitmap_start;
        let mut buffer = [0u8; 512];
        let mut file = self.file.borrow_mut();

        for chunk_idx in 0..(total_bytes / 512 + 1) {
            let current_offset = self.sb.data_bitmap_start + (chunk_idx * 512);
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
        Err(Error::other("No more free blocks!"))
    }

    fn allocate_indirect_or_direct_blocks(&self, block_index: u32) -> io::Result<u32> {
        let mut inode = self.get_inode()?;

        if block_index < 10 {
            let direct_block = inode.direct_blocks[block_index as usize];
            if direct_block == 0 {
                let new_block_id = self.allocate_data_block()?;
                inode.direct_blocks[block_index as usize] = new_block_id;
                self.save_inode(&inode)?;
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
                    "File is too large! Maximum {} blocks!",
                    10 + max_pointers_per_block
                ),
            ));
        }

        if inode.indirect_blocks == 0 {
            let new_pointer_block = self.allocate_data_block()?;
            inode.indirect_blocks = new_pointer_block;
            self.save_inode(&inode)?;
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

    fn just_read(&self, inode: &Inode, block_index: u32) -> io::Result<Option<u32>> {
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
}

impl Write for VfsFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut inode = self.get_inode()?;
        if inode.is_valid == 1 {
            inode.is_valid = 0;
            self.save_inode(&inode)?;
            self.file.borrow_mut().sync_all()?;
        }
        let block_idx = (self.position / BLOCK_SIZE as u64) as u32;
        let offset = (self.position % BLOCK_SIZE as u64) as usize;
        let physical_block_id = self.allocate_indirect_or_direct_blocks(block_idx)?;
        let disk_pos = self.sb.data_blocks_start
            + (physical_block_id as u64 * BLOCK_SIZE as u64)
            + offset as u64;

        let space_left_in_block = BLOCK_SIZE - offset;
        let to_write = std::cmp::min(space_left_in_block, buf.len());

        {
            let mut file = self.file.borrow_mut();
            file.seek(SeekFrom::Start(disk_pos))?;
            file.write_all(&buf[..to_write])?;
            file.sync_all()?;
        }
        self.position += to_write as u64;
        let mut inode = self.get_inode()?;

        if self.position > inode.size {
            inode.size = self.position;
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| Error::other(e.to_string()))?
            .as_secs();

        inode.modified_at = now;
        inode.is_valid = 1;

        self.save_inode(&inode)?;
        self.file.borrow_mut().sync_all()?;

        Ok(to_write)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.borrow_mut().sync_all()
    }
}

impl Read for VfsFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let inode = self.get_inode()?;
        if self.position >= inode.size {
            return Ok(0);
        }

        let block_idx = (self.position / BLOCK_SIZE as u64) as u32;
        let offset = (self.position % BLOCK_SIZE as u64) as usize;

        let block_id = match self.just_read(&inode, block_idx)? {
            Some(id) => id,
            None => {
                let to_read = std::cmp::min(BLOCK_SIZE - offset, buf.len());
                let available_in_file = (inode.size - self.position) as usize;
                let final_read = std::cmp::min(to_read, available_in_file);
                buf[..final_read].fill(0);
                self.position += final_read as u64;
                return Ok(final_read);
            }
        };

        let disk_pos =
            self.sb.data_blocks_start + (block_id as u64 * BLOCK_SIZE as u64) + offset as u64;

        let available_in_file = inode.size - self.position;
        let available_in_block = BLOCK_SIZE as u64 - offset as u64;
        let to_read = std::cmp::min(
            std::cmp::min(available_in_block, available_in_file) as usize,
            buf.len(),
        );

        let mut file = self.file.borrow_mut();
        file.seek(SeekFrom::Start(disk_pos))?;
        file.read_exact(&mut buf[..to_read])?;

        self.position += to_read as u64;
        Ok(to_read)
    }
}

impl Seek for VfsFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let inode = self.get_inode()?;

        let new_position: i64 = match pos {
            SeekFrom::Start(n) => n as i64,
            SeekFrom::Current(n) => self.position as i64 + n,
            SeekFrom::End(n) => inode.size as i64 + n,
        };

        if new_position < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Negative position in file!",
            ));
        }

        self.position = new_position as u64;
        Ok(self.position)
    }
}
