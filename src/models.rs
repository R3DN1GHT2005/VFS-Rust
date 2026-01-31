pub const BLOCK_SIZE: usize = 4096;
pub const MAX_NAME_LEN: usize = 32;
pub const KEY: u64 = u64::from_be_bytes(*b"Moisa%$!");
pub const INODE_SIZE: usize = 80;
pub const DIR_SIZE: usize = 40;
pub const SUPERBLOCK_SIZE: usize = 48;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct SuperBlock {
    pub key: u64,
    pub block_size: u32,
    pub total_blocks: u32,
    pub inode_bitmap_start: u64,
    pub data_bitmap_start: u64,
    pub inode_table_start: u64,
    pub data_blocks_start: u64,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct Inode {
    pub inode_type: u8,
    pub is_valid: u8,
    pub size: u64,
    pub created_at: u64,
    pub modified_at: u64,
    pub direct_blocks: [u32; 10],
    pub indirect_blocks: u32,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct DirEntry {
    pub inode_id: u32,
    pub name: [u8; MAX_NAME_LEN],
    pub is_active: u8,
}

impl SuperBlock {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(SUPERBLOCK_SIZE);
        buffer.extend_from_slice(&self.key.to_le_bytes());
        buffer.extend_from_slice(&self.block_size.to_le_bytes());
        buffer.extend_from_slice(&self.total_blocks.to_le_bytes());
        buffer.extend_from_slice(&self.inode_bitmap_start.to_le_bytes());
        buffer.extend_from_slice(&self.data_bitmap_start.to_le_bytes());
        buffer.extend_from_slice(&self.inode_table_start.to_le_bytes());
        buffer.extend_from_slice(&self.data_blocks_start.to_le_bytes());
        buffer
    }

    pub fn from_bytes(data: &[u8]) -> Self {
        Self {
            key: u64::from_le_bytes(data[0..8].try_into().unwrap()),
            block_size: u32::from_le_bytes(data[8..12].try_into().unwrap()),
            total_blocks: u32::from_le_bytes(data[12..16].try_into().unwrap()),
            inode_bitmap_start: u64::from_le_bytes(data[16..24].try_into().unwrap()),
            data_bitmap_start: u64::from_le_bytes(data[24..32].try_into().unwrap()),
            inode_table_start: u64::from_le_bytes(data[32..40].try_into().unwrap()),
            data_blocks_start: u64::from_le_bytes(data[40..48].try_into().unwrap()),
        }
    }
}

impl Inode {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(INODE_SIZE);

        bytes.push(self.inode_type);
        bytes.push(self.is_valid);
        bytes.extend_from_slice(&[0u8; 6]);

        bytes.extend_from_slice(&self.size.to_le_bytes());
        bytes.extend_from_slice(&self.created_at.to_le_bytes());
        bytes.extend_from_slice(&self.modified_at.to_le_bytes());

        for block in self.direct_blocks {
            bytes.extend_from_slice(&block.to_le_bytes());
        }

        bytes.extend_from_slice(&self.indirect_blocks.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 4]);

        bytes
    }
    pub fn from_bytes(data: &[u8]) -> Self {
        Self {
            inode_type: data[0],
            is_valid: data[1],
            size: u64::from_le_bytes(data[8..16].try_into().unwrap()),
            created_at: u64::from_le_bytes(data[16..24].try_into().unwrap()),
            modified_at: u64::from_le_bytes(data[24..32].try_into().unwrap()),
            direct_blocks: {
                let mut blocks = [0u32; 10];
                for (i, block) in blocks.iter_mut().enumerate() {
                    let start = 32 + (i * 4);
                    *block = u32::from_le_bytes(data[start..start + 4].try_into().unwrap());
                }
                blocks
            },
            indirect_blocks: u32::from_le_bytes(data[72..76].try_into().unwrap()),
        }
    }
}

impl DirEntry {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(DIR_SIZE);
        bytes.extend_from_slice(&self.inode_id.to_le_bytes());
        bytes.extend_from_slice(&self.name);
        bytes.push(self.is_active);
        bytes.extend_from_slice(&[0u8; 3]);
        bytes
    }
    pub fn from_bytes(data: &[u8]) -> Self {
        let mut name = [0u8; MAX_NAME_LEN];
        name.copy_from_slice(&data[4..36]);

        Self {
            inode_id: u32::from_le_bytes(data[0..4].try_into().unwrap()),
            name,
            is_active: data[36],
        }
    }
}
