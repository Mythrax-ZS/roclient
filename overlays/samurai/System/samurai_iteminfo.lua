-- Samurai's starter weapon. Resource names are CP949 bytes expressed as Lua
-- decimal escapes so this file remains ASCII and survives editor encoding changes.
-- Both resources refer to the existing Katana icon in the installed game data.
tbl = {
    [900000] = {
        unidentifiedDisplayName = "Unidentified Katana",
        unidentifiedResourceName = "\196\171\197\184\179\170",
        unidentifiedDescriptionName = { "Identify this weapon with a Magnifier." },
        identifiedDisplayName = "Samurai Initiate's Katana",
        identifiedResourceName = "\196\171\197\184\179\170",
        identifiedDescriptionName = {
            "A training katana awarded to a newly sworn Samurai.",
            "^0000CCType:^000000 One-Handed Sword",
            "^0000CCAttack:^000000 160",
            "^0000CCWeight:^000000 100",
            "^0000CCWeapon Level:^000000 3",
            "^0000CCRequired Base Level:^000000 99",
            "^0000CCJobs:^000000 Samurai",
            "Can be refined. Can be equipped in either hand.",
        },
        slotCount = 0,
        ClassNum = 2,
        costume = false,
    },
}
