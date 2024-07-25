use crate::types::BlockHeight;
use crate::types::BlockInfo;
use anyhow::Context;
use anyhow::Ok;
use anyhow::Result;
use bitcoin::hash_types::BlockHash;
use mockall::automock;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Write;

#[derive(Clone)]
pub struct Store {
    file_path: String,
}

impl Store {
    pub fn new(file_path: &str) -> Result<Self> {
        Ok(Self {
            file_path: String::from(file_path),
        })
    }

    pub fn get_data<T>(&mut self, file_name: &str) -> Result<Vec<T>>
    where
        T: DeserializeOwned,
    {
        let full_path = format!("{}/{}.json", self.file_path, file_name);
        let mut file = File::open(full_path).context("Error opening file")?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        let data: Vec<T> = serde_json::from_str(&contents).context("Error deserializing data")?;
        Ok(data)
    }

    fn write_data<T>(&mut self, file_name: &str, data: &Vec<T>) -> Result<()>
    where
        T: Serialize,
    {
        let full_path = format!("{}/{}.json", self.file_path, file_name);

        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true) // Truncate the file (clear existing content)
            .open(full_path)
            .context("Error opening file")?;

        let json_data = serde_json::to_string_pretty(data)?;

        file.write_all(json_data.as_bytes())?;

        Ok(())
    }
}

pub trait StoreClient {
    fn get_best_block_height(&mut self) -> Result<Option<BlockHeight>>;

    fn get_block_hash_by_height(&mut self, height: BlockHeight) -> Result<Option<BlockHash>>;

    /// Get the block by id, along with id of the previous block hash
    fn get_block_by_hash(&mut self, hash: &BlockHash) -> Result<Option<BlockInfo>>;

    fn save_block(&mut self, block: &BlockInfo) -> Result<()>;
}

#[automock]
impl StoreClient for Store {
    fn save_block(&mut self, block: &BlockInfo) -> Result<()> {
        let mut blocks: Vec<BlockInfo> = self.get_data::<BlockInfo>("blocks")?;

        let new_block = block.clone();

        if let Some(pos) = blocks.iter().position(|b| b.height == block.height) {
            blocks[pos] = new_block
        } else {
            blocks.insert(0, new_block);
        }

        self.write_data::<BlockInfo>("blocks", &blocks)?;

        Ok(())
    }

    fn get_best_block_height(&mut self) -> Result<Option<BlockHeight>> {
        let blocks: Vec<BlockInfo> = self
            .get_data::<BlockInfo>("blocks")
            .context("There was an error trying to call get_best_block_height in Store")?;

        if blocks.is_empty() {
            return Ok(None);
        }

        //Invariant: blocks ordered by height, 0 is the best block
        Ok(Some(blocks[0].height))
    }

    fn get_block_hash_by_height(&mut self, height: BlockHeight) -> Result<Option<BlockHash>> {
        let blocks: Vec<BlockInfo> = self
            .get_data::<BlockInfo>("blocks")
            .context("There was an error trying to call get_block_hash_by_height in Store")?;
        let block = blocks.iter().find(|b| b.height == height).map(|b| b.hash);
        Ok(block)
    }

    fn get_block_by_hash(&mut self, hash: &BlockHash) -> Result<Option<BlockInfo>> {
        let blocks: Vec<BlockInfo> = self
            .get_data::<BlockInfo>("blocks")
            .context("There was an error trying to call get_block_by_hash in Store")?;
        let block = blocks.into_iter().find(|b| b.hash == *hash);
        Ok(block)
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn get_data() -> Result<(), anyhow::Error> {
        let mut store = Store::new("data")?;
        let height = store.get_best_block_height()?;
        println!("best block {:?}", height);

        let block_hash = store.get_block_hash_by_height(2)?;
        println!("block hash {:?}", block_hash.unwrap());

        let block_hash =
            BlockHash::from_str("0000000000000000000b1e2b6f1f3b7f0b1f1e2b6f1f3b7f0b1f1e2b6f1f3b7f")
                .unwrap();
        let block = store.get_block_by_hash(&block_hash)?;

        println!("block hash {:?}", block.unwrap());

        let block_3 = BlockInfo {
            height: 4,
            hash: block_hash,
            prev_hash: block_hash,
        };

        store.save_block(&block_3)?;

        Ok(())
    }
}
