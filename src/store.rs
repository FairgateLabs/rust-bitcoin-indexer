use crate::types::Block;
use crate::types::BlockHeight;
use crate::types::BlockInfo;
use anyhow::Context;
use anyhow::Ok;
use anyhow::Result;
use bitcoin::hash_types::BlockHash;
use bitcoin::Txid;
use mockall::automock;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Read;
use std::io::Write;
use std::path::Path;

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

    pub fn get_data<T>(&self, file_name: &str) -> Result<Vec<T>>
    where
        T: DeserializeOwned + Serialize,
    {
        let full_path = format!("{}/{}.json", self.file_path, file_name);
        let file_exists = Path::new(&full_path).exists();

        if !file_exists {
            let _ = File::create(&full_path).with_context(|| {
                format!("File path not found. Error creating the file {}", full_path)
            })?;
            let empty_blocks = Vec::<T>::new();
            let _ = self.write_data(file_name, &empty_blocks);
        }

        let mut file =
            File::open(&full_path).with_context(|| format!("Error opening file {}", full_path))?;

        let mut contents = String::new();

        file.read_to_string(&mut contents)?;

        let data: Vec<T> = serde_json::from_str(&contents).context("Error deserializing data")?;
        Ok(data)
    }

    fn write_data<T>(&self, file_name: &str, data: &Vec<T>) -> Result<()>
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
    fn get_best_block_height(&self) -> Result<Option<BlockHeight>>;

    fn get_block_hash_by_height(&self, height: BlockHeight) -> Result<Option<BlockHash>>;

    /// Get the block by id, along with id of the previous block hash
    fn get_block_by_hash(&self, hash: &BlockHash) -> Result<Option<Block>>;

    fn save_block(&self, block: &BlockInfo) -> Result<()>;

    fn tx_exists(&self, tx_id: &Txid) -> Result<(bool, Option<BlockHeight>)>;
}

#[automock]
impl StoreClient for Store {
    fn save_block(&self, block: &BlockInfo) -> Result<()> {
        let mut blocks: Vec<Block> = self.get_data::<Block>("blocks")?;

        let new_block = Block {
            height: block.height,
            hash: block.hash,
            prev_hash: block.prev_hash,
            txs: block.txs.clone(),
            orphan: false,
        };

        if let Some(pos) = blocks.iter().position(|b| b.height == new_block.height) {
            let block_hash = blocks[pos].hash;
            let new_block_hash = new_block.hash;

            if block_hash != new_block_hash {
                blocks[pos].orphan = true;
                blocks.insert(pos + 1, new_block);
            } else {
                blocks[pos] = new_block;
            }
        } else {
            blocks.insert(0, new_block);
        }

        self.write_data::<Block>("blocks", &blocks)?;

        Ok(())
    }

    fn get_best_block_height(&self) -> Result<Option<BlockHeight>> {
        let blocks: Vec<Block> = self
            .get_data::<Block>("blocks")
            .context("There was an error trying to call get_best_block_height in Store")?;

        if blocks.is_empty() {
            return Ok(None);
        }

        // Invariant: blocks are ordered by height, 0 is the best block
        let last_height_block = Some(blocks[0].height);

        Ok(last_height_block)
    }

    fn get_block_hash_by_height(&self, height: BlockHeight) -> Result<Option<BlockHash>> {
        let blocks: Vec<Block> = self
            .get_data::<Block>("blocks")
            .context("There was an error trying to call get_block_hash_by_height in Store")?;
        let block = blocks
            .iter()
            .find(|b| b.height == height && !b.orphan)
            .map(|b| b.hash);
        Ok(block)
    }

    fn get_block_by_hash(&self, hash: &BlockHash) -> Result<Option<Block>> {
        let blocks: Vec<Block> = self
            .get_data::<Block>("blocks")
            .context("There was an error trying to call get_block_by_hash in Store")?;
        let block = blocks.into_iter().find(|b| b.hash == *hash && !b.orphan);

        Ok(block)
    }

    fn tx_exists(&self, tx_id: &Txid) -> Result<(bool, Option<BlockHeight>)> {
        let blocks: Vec<Block> = self
            .get_data::<Block>("blocks")
            .context("There was an error trying to call tx_exists in Store")?;

        let block = blocks.into_iter().find(|b| b.txs.contains(tx_id));

        if block.is_none() {
            return Ok((false, None));
        }

        let tx_height = block.unwrap().height;
        Ok((true, Some(tx_height)))
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use super::*;

    #[test]
    #[ignore]
    fn get_data() -> Result<(), anyhow::Error> {
        let store = Store::new("data")?;
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
            txs: vec![],
        };

        store.save_block(&block_3)?;

        Ok(())
    }
}
