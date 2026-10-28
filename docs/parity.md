# Parity checklist

Server packets rAthena can send at PACKETVER 20250402 that the client does not handle yet,
found by comparing `ro-proto/src/generated.rs` with the opcodes in `ro-net` and `ro-proto`.
Ticked items are handled and tested in game against the local server.

## Scripting (NPC script commands)
- [x] ZC_SOUND (`soundeffect`), ZC_PLAY_NPC_BGM (`playBGM`)
- [x] ZC_COMPASS (`viewpoint`: minimap marks)
- [x] ZC_NPCSPRITE_CHANGE (`setnpcdisplay`)
- [x] ZC_CHANGE_DIRECTION; server directions (0 north) converted to sprite directions (0 south): standing units faced the wrong way
- [x] ZC_DIALOG_TEXT_ALIGN / WINDOW_SIZE / WINDOW_POS / POS2 (ZC_SAY_DIALOG2 / WAIT_DIALOG2 are never sent by rAthena)
- [x] ZC_MSG_COLOR, ZC_MSG_VALUE, ZC_MSG_SKILL, and every ZC_MSG id (was: only two)
- [x] ZC_SPECIALPOPUP (System/spopup.lub: chat and/or floating banner), ZC_SKILLMSG
- [x] ZC_REMOVE_EFFECT, ZC_EQUIPMENT_EFFECT
- [ ] ZC_NOTIFY_EFFECT3
- [x] ZC_NPC_MARKET_OPEN / PURCHASE_RESULT (market shops)
- [x] ZC_NPC_BARTER_MARKET_ITEMINFO / EXPANDED_BARTER (barter shops)
- [x] Stylist (openstylist: hair, hair colour, clothes colour, live preview)
- [x] ZC_DRESSROOM_OPEN (opendressroom: preview only)
- [x] ZC_ADD_SKILL, ZC_SKILLINFO_UPDATE2, ZC_SKILLINFO_DELETE
- [x] Random enchant (ZC_UI_OPEN_V3 + ZC_RESPONSE_ENCHANT) from the official enchantlist.lub (32-bit bytecode widened and run)
- [x] Item reform (ZC_OPEN_REFORM_UI / ITEM_REFORM_ACK) from itemreformsystem.lub
- [x] Failed item adds (full inventory, overweight) now say why
- [x] Reputation window (ZC_REPUTE_INFO / OPEN, names from contentdataepute*.bson)
- [~] Item merge window (mergeitem): built; no mergeable stacks could be produced to test
- [ ] ZC_RANDOM_COMBINE / RANDOM_UPGRADE_ITEM

## Items and skills
- [x] ZC_ACK_ITEMIDENTIFY (Magnifier) + ZC_AUTORUN_SKILL (all itemskill items)
- [ ] ZC_REPAIRITEMLIST / ACK_ITEMREPAIR
- [~] ZC_ACK_WEAPONREFINE message; ZC_NOTIFY_WEAPONITEMLIST list still missing
- [x] ZC_ACK_REMEMBER_WARPPOINT (Warp Portal memo)
- [x] ZC_EQUIP_ARROW, ZC_ACTION_FAILURE, ZC_ACK_ADDITEM_TO_CART, ZC_NOTIFY_MAPINFO, ZC_EQUIPITEM_DAMAGED
- [x] ZC_ATTACK_RANGE, ZC_OVERWEIGHT_PERCENT, ZC_MAPPROPERTY_R2 (PvP/GvG: click enemy players to attack, Shift when the map requires it)
- [ ] ZC_CARTOFF
- [x] ZC_SPIRITS (spheres circling the unit)
- [ ] ZC_SPIRITS2 / SPIRITS_ATTRIBUTE / SOULENERGY / TARGET_SPIRITS, ZC_MILLENNIUMSHIELD
- [ ] ZC_COMBODELAY, ZC_BLADESTOP, ZC_DISPEL, ZC_HIGHJUMP, ZC_NOTIFY_SKILL_POSITION
- [ ] ZC_NOTIFY_BIND_ON_EQUIP, ZC_CHANGE_ITEM_OPTION, ZC_UPDATE_CARDSLOT
- [ ] ZC_ACK_TAKEOFF_EQUIP_ALL, ZC_MOVE_ITEM_FAILED, ZC_INVENTORY_TAB, ZC_INVENTORY_END
- [ ] ZC_EXTEND_BODYITEM_SIZE (+ ACK / MSGBOX)

## World
- [~] ZC_MVP, ZC_MVP_GETTING_ITEM, ZC_MVP_GETTING_SPECIAL_EXP, ZC_THROW_MVPITEM (built; not seen in game yet)
- [x] ZC_NOTIFY_POSITION_TO_GROUPM / GUILDM (party members off-screen on the minimap)
- [ ] ZC_NOTIFY_MEMBERINFO_TO_GROUPM
- [ ] ZC_NOTIFY_MAPPROPERTY2, ZC_NOTIFY_MAPINFO, ZC_UPDATE_MAPINFO, ZC_MONSTER_INFO, ZC_BOSS_INFO
- [ ] ZC_CHANGE_DIRECTION, ZC_SKILL_SCALE, ZC_NOTIFY_ACTORINIT, ZC_COUPLENAME
- [x] Taming (ZC_START_CAPTURE / TRYCAPTURE_MONSTER)
- [ ] ZC_FEED_PET, ZC_PET_ACT, ZC_FEED_MER
- [ ] ZC_SUMMON_HP_INIT / UPDATE, ZC_HO_PAR_CHANGE, ZC_EL_PAR_CHANGE, ZC_HOSKILLINFO_UPDATE

## Social
- [x] Guild emblems in the guild window (download, upload from Emblem*.bmp)
- [x] Guild emblems beside player names (guild id from spawn packets, fetched per guild)
- [ ] ZC_GUILD_POSITION, ZC_CHANGE_GUILD
- [x] Guild alliances and enemies (request, answer, list in the guild window)
- [ ] Guild disorganize
- [x] ZC_CONFIG, ZC_PARTY_CONFIG, party options: Game Options window
- [ ] ZC_PARTY_JOIN_REQ_ACK, ZC_ITEM_PICKUP_PARTY
- [x] Store search (Universal Catalog: search, pages, click to locate)
- [ ] ZC_ACK_OPENSTORE2
- [x] Ranking window (ZC_ACK_RANKING2 + names by char id; /blacksmith /alchemist /taekwon /pk)
- [ ] ZC_CONGRATULATION, ZC_DIVORCE
- [ ] ZC_BAN_LIST

## Found while testing
- [x] `specialeffect` (ZC_NOTIFY_EFFECT2): 110 EF_* ids matched to STR files, plus the most-used code-drawn ones (heal, grand cross, steal, bash, ...) shown with their skill effect; the hit sparks and screen quake still missing
- [ ] Some NPC classes have no sprite name (e.g. 10243 4_m_drzonda01)
- Note: `setnpcdisplay` to a monster class makes rAthena send class 0 (a Novice); the original client shows the same
- [x] ZC_ACK_REQNAMEALL: party, guild and rank under player names
- [x] ZC_EQUIPMENT_EFFECT: hat effects (145 from hateffectinfo), following the wearer
- [x] Equipment switch set (Switch tab: add, remove, swap; ZC_EQUIPSWITCH_LIST/ADD/REMOVE/REPLY)
