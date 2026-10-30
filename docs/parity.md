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
- [x] ZC_NOTIFY_EFFECT3 (0x0284 / 0x0b69: effect plus green heal numbers)
- [x] ZC_NPC_MARKET_OPEN / PURCHASE_RESULT (market shops)
- [x] ZC_NPC_BARTER_MARKET_ITEMINFO / EXPANDED_BARTER (barter shops)
- [x] Stylist (openstylist: hair, hair colour, clothes colour, live preview)
- [x] ZC_DRESSROOM_OPEN (opendressroom: preview only)
- [x] ZC_ADD_SKILL, ZC_SKILLINFO_UPDATE2, ZC_SKILLINFO_DELETE
- [x] Random enchant (ZC_UI_OPEN_V3 + ZC_RESPONSE_ENCHANT) from the official enchantlist.lub (32-bit bytecode widened and run)
- [x] Item reform (ZC_OPEN_REFORM_UI / ITEM_REFORM_ACK) from itemreformsystem.lub
- [x] Failed item adds (full inventory, overweight) now say why
- [x] Reputation window (ZC_REPUTE_INFO / OPEN, names from contentdata
epute*.bson)
- [~] Item merge window (mergeitem): built; no mergeable stacks could be produced to test
- [x] ZC_RANDOM_COMBINE / RANDOM_UPGRADE_ITEM (Laphine synthesis + upgrade windows, recipes from lapine*.lub merged across iRO/kRO; ZC_CHANGE_ITEM_OPTION 0x0b43)

## Items and skills
- [x] ZC_ACK_ITEMIDENTIFY (Magnifier) + ZC_AUTORUN_SKILL (all itemskill items)
- [x] ZC_REPAIRITEMLIST / ACK_ITEMREPAIR (Weapon Repair list and result)
- [~] ZC_ACK_WEAPONREFINE message; ZC_NOTIFY_WEAPONITEMLIST list still missing
- [x] ZC_ACK_REMEMBER_WARPPOINT (Warp Portal memo)
- [x] ZC_EQUIP_ARROW, ZC_ACTION_FAILURE, ZC_ACK_ADDITEM_TO_CART, ZC_NOTIFY_MAPINFO, ZC_EQUIPITEM_DAMAGED
- [x] ZC_ATTACK_RANGE, ZC_OVERWEIGHT_PERCENT, ZC_MAPPROPERTY_R2 (PvP/GvG: click enemy players to attack, Shift when the map requires it)
- [x] ZC_CARTOFF
- [x] ZC_SPIRITS (spheres circling the unit)
- [x] ZC_SPIRITS2 / SPIRITS_ATTRIBUTE / SOULENERGY, ZC_MILLENNIUMSHIELD (orbs coloured by kind)
- [x] ZC_TARGET_SPIRITS (Enchanting Shadow orbs, dark violet)
- [x] ZC_DISPEL (cast cancel), ZC_HIGHJUMP (slide); ZC_NOTIFY_SKILL_POSITION unused by rAthena
- [x] ZC_COMBODELAY (tracked), ZC_BLADESTOP (both units held in place) — untested in game
- [x] ZC_NOTIFY_BIND_ON_EQUIP (chat notice)
- [x] ZC_CHANGE_ITEM_OPTION (0x0b43, Laphine upgrade); ZC_UPDATE_CARDSLOT not sent by rAthena
- [x] ZC_ACK_TAKEOFF_EQUIP_ALL ("Take off all" button per equipment tab), ZC_MOVE_ITEM_FAILED
- [x] ZC_INVENTORY_TAB (own fourth "fav" tab; the server flag is PlaceETCTab); ZC_INVENTORY_END is a no-op marker
- [x] ZC_EXTEND_BODYITEM_SIZE (+ ACK / MSGBOX): slot count, + button, confirmation

## World
- [~] ZC_MVP, ZC_MVP_GETTING_ITEM, ZC_MVP_GETTING_SPECIAL_EXP, ZC_THROW_MVPITEM (built; not seen in game yet)
- [x] ZC_NOTIFY_POSITION_TO_GROUPM / GUILDM (party members off-screen on the minimap)
- [x] ZC_NOTIFY_MEMBERINFO_TO_GROUPM
- [x] ZC_NOTIFY_MAPINFO, ZC_BOSS_INFO (Convex Mirror: chat + minimap mark)
- [x] ZC_UPDATE_MAPINFO (setcell/setwall change client pathing), ZC_MONSTER_INFO (Sense window)
- [x] ZC_NOTIFY_MAPPROPERTY2 (battleground map type; consumed, nothing to draw)
- [x] ZC_CHANGE_DIRECTION
- [x] ZC_COUPLENAME (Call Partner notice)
- [x] ZC_NOTIFY_ACTORINIT (consumed); ZC_SKILL_SCALE not sent by rAthena
- [x] Taming (ZC_START_CAPTURE / TRYCAPTURE_MONSTER)
- [x] ZC_FEED_PET, ZC_FEED_MER (chat result)
- [x] ZC_PET_ACT (pettalktable.xml lines in chat, emotions)
- [x] ZC_SUMMON_HP_INIT / UPDATE, ZC_HO_PAR_CHANGE2, ZC_HOSKILLINFO_UPDATE (summon bar tested with ABR Battle Warrior)
- [x] Elementals (ZC_EL_INIT, ZC_EL_PAR_CHANGE): companion window tab + HP bar

## Social
- [x] Guild emblems in the guild window: BMP, GIF or PNG (with alpha) up to 256x256 from the Emblem folder, uploaded to and downloaded from the web server (/emblem/*, CZ_REQ_ADD_NEW_EMBLEM) as 2019+ clients do; 24x24 BMPs over the game connection when there is no web server
- [x] Guild emblems beside player names (guild id from spawn packets, fetched per guild)
- [x] ZC_CHANGE_GUILD
- [x] ZC_GUILD_POSITION (rank title under names updates live)
- [x] Guild alliances and enemies (request, answer, list in the guild window)
- [x] Guild disorganize
- [x] ZC_CONFIG, ZC_PARTY_CONFIG, party options: Game Options window
- [x] ZC_PARTY_JOIN_REQ_ACK, ZC_ITEM_PICKUP_PARTY
- [x] Store search (Universal Catalog: search, pages, click to locate)
- [x] ZC_ACK_OPENSTORE2
- [x] Ranking window (ZC_ACK_RANKING2 + names by char id; /blacksmith /alchemist /taekwon /pk)
- [x] ZC_CONGRATULATION, ZC_DIVORCE (chat notices; no wedding effect yet)
- [x] ZC_BAN_LIST

## Found while testing
- [x] `specialeffect` (ZC_NOTIFY_EFFECT2): 110 EF_* ids matched to STR files, plus the most-used code-drawn ones (heal, grand cross, steal, bash, ...) shown with their skill effect; the hit sparks and screen quake still missing
- [x] NPC classes whose sprite iRO lacks (e.g. 10243 4_m_drzonda01, Zonda teleporters) get a stand-in body so they stay usable
- Note: `setnpcdisplay` to a monster class makes rAthena send class 0 (a Novice); the original client shows the same
- [x] ZC_ACK_REQNAMEALL: party, guild and rank under player names
- [x] ZC_EQUIPMENT_EFFECT: hat effects (145 from hateffectinfo), following the wearer
- [x] Equipment switch set (Switch tab: add, remove, swap; ZC_EQUIPSWITCH_LIST/ADD/REMOVE/REPLY)
- [x] Level aura for players reported at level 99+ (clevel from the spawn packets; EF_LEVEL99 ribbons, EF_LEVEL99_2 ground star, EF_LEVEL99_3 sparkles, code-drawn after roBrowser's reconstruction); /aura simplifies it, /aura2 turns it off. rAthena's `max_lv` / `aura_lv` / `client_limit_unit_lv` choose who gets it
- [ ] The higher auras (EF_LEVEL99_150, the 4th-job orbs EF_LEVEL99_ORB1/2): when the original shows them is unknown
- [x] Emotion List (Alt+L, /emo): every emote with its picture and command, click to show it. The emotion sprite's actions are not in ET_* order (`emotes::action`, as roBrowser's table), which also fixed the emotes drawn over heads
- [x] Macro window (Alt+M, "Shortcut List"): what Alt+1..Alt+0 say, the first ten emotes by default; any emote, phrase or / command. The minimap toggle moved to Ctrl+Tab, as in the original
- [x] Battle mode (/bm, /battlemode, msgstringtable 785-796): Q ~ O, A ~ L and Z ~ > use the second, third and fourth shortcut rows; Enter or Space opens the chat and sending a line gives the keys back. Outside it, typing over the game goes to the chat and the chat keeps the keyboard, while F1..F12 and Alt shortcuts still work
- [x] Shortcut bar rows: F12 shows one to four rows or hides the bar (the original's extend); the page button turns which row comes first. Rows above the first are labelled with their battle mode keys
- Unverified: the original's strings say Z ~ > is shortcut window 1 (the 3-row era); with four rows this client uses it for the fourth. The original's later key-binding window (Shortcut Settings, MSI_HOTKEYWND) is not done
- Deliberate difference: Prontera's south avenue paving (`src/mapfix.rs`). The map mirrors its cobble rosettes across the avenue's centre line but sets the mirrored tiles half a rosette lower, so the original shows a seam down the middle; this client aligns them (307 tiles, ground drawing only)

## Packet audit (2026-10-30)
Server packets for PACKETVER 20250402 were taken from rAthena's preprocessed packet
headers and clif.cpp, then compared with what this client reads and sends.
- [x] CZ_REQ_DISCONNECT / ZC_ACK_REQ_DISCONNECT: Exit asks the server; "Please wait 10 seconds before trying to log out." (also for a refused character select)
- [x] 4th-job trait stats (POW..CRT, P.Atk..C.Rate, trait points; CZ_ADVANCED_STATUS_CHANGE) and the AP gauge
- [x] CZ_CHANGE_GROUP_MASTER: delegate party leadership from a member's right-click menu
- [x] CZ_STANDING_RESURRECTION: "Revive" in the death window with a Token of Siegfried or Light of Regene
- [x] CZ_REQ_ONECLICK_ITEMIDENTIFY: identify with a carried Magnifier
- [x] CZ_USE_SKILL_START / END: hold Rolling Cutter's shortcut key
- [x] Inventory items dragged onto the world are dropped (stacks ask how many)
- [ ] Selectable item packages (CZ_USE_PACKAGEITEM; client table selectpackageitem.lub, box names untranslated)
- [ ] Super Novice /doridori and the 99% chant (CZ_DORIDORI, CZ_CHOPOKGI)
- [ ] Pet emotions from the pet window (CZ_PET_ACT)
- [ ] /changedress (CZ_REQ_CHANGEDRESS), stop-attack (CZ_CANCEL_LOCKON), facing changes (CZ_CHANGE_DIRECTION)
- [ ] Storage favourites tab (CZ_MOVE_ITEM_FROM_STORE_TO_FAVORITE), skill reset (CZ_RESET_SKILL), private airship
- [x] GM tools: /b /nb /lb /nlb /hide /mm /recall /shift /item /monster /rc /check /killall /resetstate /resetcooltime /changemaptype, and GM right-click actions (config `gm`)
- [ ] GM macro-detector captcha registration and preview (CZ_UPLOAD_MACRO_DETECTOR_CAPTCHA, CZ_REQ_PREVIEW_MACRO_DETECTOR)
- Not planned: auction house, pre-RODEX mail and the old party booking (replaced at this client version); /resetskill (CZ_RESET type 1 is compiled out of rAthena at this version) and account-name lookup (rAthena always answers with an empty name)
