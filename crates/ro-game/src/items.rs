//! Item names, descriptions and icon resource names from the client's iteminfo.lub,
//! plus the player's inventory.

use std::collections::{BTreeMap, HashMap};

use ro_formats::lub::{self, Value};

#[derive(Debug, Clone, Default)]
pub struct ItemInfo {
    pub name: String,
    /// File name (without extension) under texture\유저인터페이스\item\ and collection\.
    pub resource: String,
    pub description: Vec<String>,
    pub unidentified_name: String,
    pub unidentified_resource: String,
    pub slots: u8,
    /// Sprite id for headgear/weapon views.
    pub view: u32,
}

#[derive(Default)]
pub struct ItemDb {
    items: HashMap<u32, ItemInfo>,
}

impl ItemDb {
    /// Parses a compiled iteminfo (`tbl = { [id] = { identifiedDisplayName = ..., ... } }`).
    pub fn from_lub(data: &[u8]) -> anyhow::Result<Self> {
        let chunk = lub::eval(data)?;
        let tbl = chunk.globals.get("tbl").ok_or_else(|| anyhow::anyhow!("iteminfo has no `tbl`"))?;
        let s = |t: &Value, k: &str| chunk.field(t, k).and_then(Value::as_str).unwrap_or_default().to_string();
        let n = |t: &Value, k: &str| chunk.field(t, k).and_then(Value::as_number).unwrap_or(0.0);
        let lines = |t: &Value, k: &str| -> Vec<String> {
            chunk.field(t, k).map_or(vec![], |d| chunk.entries(d).iter().filter_map(|(_, v)| v.as_str().map(str::to_string)).collect())
        };
        let items = chunk
            .entries(tbl)
            .iter()
            .filter_map(|(id, t)| {
                let id = id.as_number()? as u32;
                Some((
                    id,
                    ItemInfo {
                        name: s(t, "identifiedDisplayName"),
                        resource: s(t, "identifiedResourceName"),
                        description: lines(t, "identifiedDescriptionName"),
                        unidentified_name: s(t, "unidentifiedDisplayName"),
                        unidentified_resource: s(t, "unidentifiedResourceName"),
                        slots: n(t, "slotCount") as u8,
                        view: n(t, "ClassNum") as u32,
                    },
                ))
            })
            .collect();
        Ok(Self { items })
    }

    /// Adds or replaces entries (a server's custom itemInfo over the stock table).
    pub fn extend(&mut self, items: impl IntoIterator<Item = (u32, ItemInfo)>) {
        self.items.extend(items);
    }

    /// Item ids whose name contains `q` (lower-case), shortest names first.
    pub fn search(&self, q: &str) -> Vec<u32> {
        let mut v: Vec<(usize, u32)> = self.items.iter().filter(|(_, i)| i.name.to_lowercase().contains(q)).map(|(id, i)| (i.name.len(), *id)).collect();
        v.sort_unstable();
        v.into_iter().map(|(_, id)| id).collect()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, id: u32) -> Option<&ItemInfo> {
        self.items.get(&id)
    }

    /// Display name, honouring identification; falls back to the id.
    pub fn name(&self, id: u32, identified: bool) -> String {
        match self.items.get(&id) {
            Some(i) if identified || i.unidentified_name.is_empty() => i.name.clone(),
            Some(i) => i.unidentified_name.clone(),
            None => format!("Item #{id}"),
        }
    }
}

/// rAthena item types (`item_types`).
pub mod kind {
    pub const HEALING: u8 = 0;
    pub const USABLE: u8 = 2;
    pub const ETC: u8 = 3;
    pub const ARMOR: u8 = 4;
    pub const WEAPON: u8 = 5;
    pub const CARD: u8 = 6;
    pub const AMMO: u8 = 10;
    pub const DELAYCONSUME: u8 = 11;
    pub const CASH: u8 = 18;
}

/// One inventory slot as the server describes it. `index` is the wire index.
#[derive(Debug, Clone, Default)]
pub struct InvItem {
    pub index: u16,
    pub id: u32,
    pub kind: u8,
    pub amount: u16,
    /// Equip slots the item can go in (0 for non-equipment).
    pub location: u32,
    /// Slots it currently occupies (0 when not worn).
    pub equipped: u32,
    pub refine: u8,
    /// Enchant grade: 0 none, 1 D, 2 C, 3 B, 4 A.
    pub grade: u8,
    pub identified: bool,
    pub cards: [u32; 4],
    /// Sprite id when worn (headgear view / weapon class).
    pub look: u16,
    /// Slots this item takes in the equipment-switch set (0: not in it).
    pub switch: u32,
    /// Kept in the "etc" (favourites) tab.
    pub favorite: bool,
}

impl InvItem {
    pub fn is_equipment(&self) -> bool {
        self.location != 0
    }

    pub fn is_usable(&self) -> bool {
        matches!(self.kind, kind::HEALING | kind::USABLE | kind::DELAYCONSUME | kind::CASH)
    }
}

#[derive(Default)]
pub struct Inventory {
    pub items: BTreeMap<u16, InvItem>,
}

impl Inventory {
    pub fn set(&mut self, item: InvItem) {
        self.items.insert(item.index, item);
    }

    /// Adds a picked-up stack (same index → more of it).
    pub fn add(&mut self, item: InvItem) {
        match self.items.get_mut(&item.index) {
            Some(i) if i.id == item.id && !item.is_equipment() => i.amount += item.amount,
            _ => self.set(item),
        }
    }

    pub fn remove(&mut self, index: u16, amount: u16) {
        if let Some(i) = self.items.get_mut(&index) {
            i.amount = i.amount.saturating_sub(amount);
            if i.amount == 0 {
                self.items.remove(&index);
            }
        }
    }

    pub fn set_amount(&mut self, index: u16, amount: u16) {
        if amount == 0 {
            self.items.remove(&index);
        } else if let Some(i) = self.items.get_mut(&index) {
            i.amount = amount;
        }
    }

    pub fn set_equipped(&mut self, index: u16, slots: u32) {
        if let Some(i) = self.items.get_mut(&index) {
            i.equipped = slots;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iteminfo_if_present() {
        let Ok(path) = std::env::var("RO_ITEMINFO") else { return };
        let db = ItemDb::from_lub(&std::fs::read(path).unwrap()).unwrap();
        assert!(db.len() > 10_000);
        let red = db.get(501).unwrap();
        assert_eq!(red.name, "Red Potion");
        assert!(!red.resource.is_empty());
    }

    #[test]
    fn stacks_and_removal() {
        let mut inv = Inventory::default();
        inv.add(InvItem { index: 2, id: 501, amount: 3, ..Default::default() });
        inv.add(InvItem { index: 2, id: 501, amount: 2, ..Default::default() });
        assert_eq!(inv.items[&2].amount, 5);
        inv.remove(2, 5);
        assert!(inv.items.is_empty());
    }
}
