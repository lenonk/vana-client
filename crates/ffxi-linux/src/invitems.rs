//! `ItemInfo` from the item DATs (dat/items.rs): names, stack, type, flags and the 32x32 icon.

use crate::dat::items::{Items, Lang};
use crate::inventory::ItemInfo;

impl ItemInfo for Items {
    fn name(&self, id: u16) -> Option<String> {
        self.item(id as u32).map(|i| i.name)
    }
    fn stack(&self, id: u16) -> u16 {
        self.item(id as u32).map_or(1, |i| i.stack)
    }
    fn kind(&self, id: u16) -> u16 {
        self.item(id as u32).map_or(0, |i| i.item_type)
    }
    fn flags(&self, id: u16) -> u16 {
        self.item(id as u32).map_or(0, |i| i.flags)
    }
    fn icon(&self, id: u16) -> Option<Vec<u8>> {
        let (w, h, rgba) = self.icon(id as u32)?;
        (w == 32 && h == 32).then_some(rgba)
    }
}

/// English item data of the install, if its files are there.
pub fn load(index: &crate::dat::DatIndex) -> Option<Box<dyn ItemInfo>> {
    Items::load(index, Lang::En).ok().map(|i| Box::new(i) as Box<dyn ItemInfo>)
}
