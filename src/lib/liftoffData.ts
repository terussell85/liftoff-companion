export type LiftoffRaceSeed = {
  name: string;
  assetKey: string;
  trackKey: string;
};

export type LiftoffLevelSeed = {
  environmentId: string;
  name: string;
  races: LiftoffRaceSeed[];
};

/**
 * Liftoff: Micro Drones shipped environment variants and official race tracks.
 * Extracted from local Unity metadata only: Environment ScriptableObject display
 * names plus embedded Race/Track XML names. No game assets are bundled here.
 */
export const MICRO_DRONES_LEVEL_DATA: LiftoffLevelSeed[] = [
  {
    environmentId: "BasketballCourt",
    name: "Hoverton High - Obstacle Course",
    races: [
      {
        name: "01 - Gym Class",
        assetKey: "HovertonHighRace01_0001",
        trackKey: "01-GymClass_0001",
      },
      {
        name: "02 - Through the Trusses",
        assetKey: "HovertonHighRace02_0001",
        trackKey: "02-ThroughtheTrusses_0001",
      },
      {
        name: "03 - Hall Pass",
        assetKey: "HovertonHighRace03_0001",
        trackKey: "03-HallPass_0001",
      },
      {
        name: "H01 - Shortcut",
        assetKey: "BasketballCourtHoverdroneRace01_0001",
        trackKey: "BasketballCourtHoverdroneTrack01_0001",
      },
    ],
  },
  {
    environmentId: "BasketballCourt_Night",
    name: "Hoverton High - Prom Night",
    races: [
      {
        name: "01 - Color Coded",
        assetKey: "HovertonHighNightRace01_0001",
        trackKey: "01-ColorCoded_0001",
      },
      {
        name: "02 - Ballroom Blitz",
        assetKey: "HovertonHighNightRace02_0001",
        trackKey: "02-BallroomBlitz_0001",
      },
      {
        name: "03 - The Punchline",
        assetKey: "HovertonHighNightRace03_0001",
        trackKey: "03-ThePunchline_0001",
      },
      {
        name: "H01 - Sneaking Backstage",
        assetKey: "BasketballCourtNightHoverdroneRace01_0001",
        trackKey: "BasketballCourtNightHoverdroneTrack01_0001",
      },
    ],
  },
  {
    environmentId: "BasketballCourt_Empty",
    name: "Hoverton High - Empty",
    races: [],
  },
  {
    environmentId: "InTransit",
    name: "In Transit - OSHA",
    races: [
      {
        name: "01 - Order Picking",
        assetKey: "InTransitRace01_0001",
        trackKey: "01-OrderPicking_0001",
      },
      {
        name: "02 - Top Shelf",
        assetKey: "InTransitRace02_0001",
        trackKey: "02-TopShelf_0001",
      },
      {
        name: "03 - Crate Inspection",
        assetKey: "InTransitRace03_0001",
        trackKey: "03-CrateInspection_0001",
      },
      {
        name: "H01 - Paint 8",
        assetKey: "InTransitHoverdroneRace_0001",
        trackKey: "H01-Paint8_0001",
      },
    ],
  },
  {
    environmentId: "InTransit_Night",
    name: "In Transit - Collapse",
    races: [
      {
        name: "01 - Working Late",
        assetKey: "InTransitNightRace01_0001",
        trackKey: "01-WorkingLate_0001",
      },
      {
        name: "02 - Collateral Damage",
        assetKey: "InTransitNightRace02_0001",
        trackKey: "02-CollateralDamage_0001",
      },
      {
        name: "03 - Risky Business",
        assetKey: "InTransitNightRace03_0001",
        trackKey: "03-RiskyBusiness_0001",
      },
      {
        name: "H01 - Drop-in",
        assetKey: "InTransitNightHoverdroneRace01_0001",
        trackKey: "H01-Drop-in_0001",
      },
    ],
  },
  {
    environmentId: "InTransit_Empty",
    name: "In Transit - Empty",
    races: [],
  },
  {
    environmentId: "JapanesePlayground",
    name: "Melon-pan Park - Playground",
    races: [
      {
        name: "01 - Childhood Memories",
        assetKey: "JapanesePlaygroundRace01_0001",
        trackKey: "01-ChildhoodMemories_0001",
      },
      {
        name: "02 - Alley Rally",
        assetKey: "JapanesePlaygroundRace02_0001",
        trackKey: "JapanesePlaygroundTrack02_0001",
      },
      {
        name: "03 - Gap Glide",
        assetKey: "JapanesePlaygroundRace03_0001",
        trackKey: "03-GapGlide_0001",
      },
      {
        name: "H01 - Detour",
        assetKey: "JapanesePlaygroundHoverdroneRace01_0001",
        trackKey: "H01-Detour_0001",
      },
    ],
  },
  {
    environmentId: "JapanesePlayground_Night",
    name: "Melon-pan Park - Neon City",
    races: [
      {
        name: "01 - Casual Drive",
        assetKey: "JapanesePlaygroundNightRace01_0001",
        trackKey: "01-CasualDrive_0001",
      },
      {
        name: "02 - Tight Spaces",
        assetKey: "JapanesePlaygroundNightRace02_0001",
        trackKey: "02-TightSpaces_0001",
      },
      {
        name: "03 - Night Lights",
        assetKey: "JapanesePlaygroundNightRace03_0001",
        trackKey: "03-NightLights_0001",
      },
      {
        name: "H01 - Hidden Away",
        assetKey: "JapanesePlaygroundNightHoverdroneRace01_0001",
        trackKey: "H01-HiddenAway_0001",
      },
      {
        name: "H02 - Pro Skater",
        assetKey: "JapanesePlaygroundNightHoverdroneRace02_0001",
        trackKey: "H02-ProSkater_0001",
      },
    ],
  },
  {
    environmentId: "JapanesePlayground_Empty",
    name: "Melon-pan Park - Empty",
    races: [],
  },
  {
    environmentId: "SawdustInc",
    name: "Sawdust Inc - Open For Business",
    races: [
      {
        name: "01 - Furniture Pick-up",
        assetKey: "SawdustIncRace01_0001",
        trackKey: "01-Furniturepick-up_0001",
      },
      {
        name: "02 - Ventilation",
        assetKey: "SawdustIncRace02_0001",
        trackKey: "02-Ventilation_0001",
      },
      {
        name: "03 - Workshop",
        assetKey: "SawdustIncRace03_0001",
        trackKey: "03-Workshop_0001",
      },
      {
        name: "H01 - Showroom",
        assetKey: "SawdustIncHoverdroneRace01_0001",
        trackKey: "H01-Showroom_0001",
      },
    ],
  },
  {
    environmentId: "SawdustInc_Night",
    name: "Sawdust Inc - Flooded",
    races: [
      {
        name: "01 - Wet Feet",
        assetKey: "SawdustIncNightRace01_0001",
        trackKey: "01-Wetfeet_0001",
      },
      {
        name: "02 - Construction Work",
        assetKey: "SawdustIncNightRace02_0001",
        trackKey: "02-ConstructionWork_0001",
      },
      {
        name: "03 - Through the gaps",
        assetKey: "SawdustIncNightRace03_0001",
        trackKey: "03-ThroughtheGaps_0001",
      },
      {
        name: "H01 - Obstacle Course",
        assetKey: "SawdustIncNightHoverdroneRace01_0001",
        trackKey: "H01-ObstacleCourse_0001",
      },
    ],
  },
  {
    environmentId: "SawdustInc_Empty",
    name: "Sawdust Inc - Empty",
    races: [],
  },
  {
    environmentId: "Sealand",
    name: "Sealand - Light Air",
    races: [
      {
        name: "01 - Calm sea",
        assetKey: "SealandRace01_0001",
        trackKey: "01-CalmSea_0001",
      },
      {
        name: "02 - To the Buoy",
        assetKey: "SealandRace02_0001",
        trackKey: "02-TotheBuoy_0001",
      },
      {
        name: "03 - Seasick",
        assetKey: "SealandRace03_0001",
        trackKey: "03-Seasick_0001",
      },
      {
        name: "H01 - You spin me round",
        assetKey: "SealandHoverdroneRace01_0001",
        trackKey: "H01-Youspinmeround_0001",
      },
    ],
  },
  {
    environmentId: "Sealand_Night",
    name: "Sealand - Howling Gale",
    races: [
      {
        name: "01 - Between the pillars",
        assetKey: "SealandNightRace01_0001",
        trackKey: "01-Betweenthepillars_0001",
      },
      {
        name: "02 - Overboard",
        assetKey: "SealandNightRace02_0001",
        trackKey: "02-Overboard_0001",
      },
      {
        name: "03 - Seaworthy",
        assetKey: "SealandNightRace03_0001",
        trackKey: "03-Seaworthy_0001",
      },
      {
        name: "H01 - Around the Deck",
        assetKey: "SealandNightHoverdroneRace01_0001",
        trackKey: "H01-AroundtheDeck_0001",
      },
    ],
  },
  {
    environmentId: "Sealand_Empty",
    name: "Sealand - Empty",
    races: [],
  },
  {
    environmentId: "SilverScreen",
    name: "Azure District - Silverscreen",
    races: [
      {
        name: "01 - Garage Galore",
        assetKey: "UndergroundParkingRace01_0001",
        trackKey: "01-GarageGalore_0001",
      },
      {
        name: "02 - Cinema Premier",
        assetKey: "UndergroundParkingRace02_0001",
        trackKey: "02-Cinemapremier_0001",
      },
      {
        name: "03 - Blockbuster",
        assetKey: "UndergroundParkingRace03_0001",
        trackKey: "03-Blockbuster_0001",
      },
      {
        name: "H01 - Looking For a Spot",
        assetKey: "UndergroundParkingHoverdroneRace01_0001",
        trackKey: "H01-Lookingforaspot_0001",
      },
    ],
  },
  {
    environmentId: "SilverScreen_Night",
    name: "Azure District - Grip",
    races: [
      {
        name: "01 - Spelunking",
        assetKey: "UndergroundParkingNightRace01_0001",
        trackKey: "01-Spelunking_0001",
      },
      {
        name: "02 - Boulder Bend",
        assetKey: "UndergroundParkingNightRace02_0001",
        trackKey: "02-BoulderBend_0001",
      },
      {
        name: "03 - Off Route",
        assetKey: "UndergroundParkingNightRace03_0001",
        trackKey: "03-OffRoute_0001",
      },
      {
        name: "H01 - In Between Climbs",
        assetKey: "UndergroundParkingNightHoverdroneRace01_0001",
        trackKey: "H01-InBetweenClimbs_0001",
      },
    ],
  },
  {
    environmentId: "SilverScreen_Empty",
    name: "Azure District - Empty",
    races: [],
  },
  {
    environmentId: "TinyHouse",
    name: "San Lipo Drive - Lockdown",
    races: [
      {
        name: "01 - In-'n'-Out",
        assetKey: "TinyHouseRace01_0001",
        trackKey: "01-In-'n'-out_0001",
      },
      {
        name: "02 - Morning Routine",
        assetKey: "TinyHouseRace02_0001",
        trackKey: "02-MorningRoutine_0001",
      },
      {
        name: "03 - Basement Dwellers",
        assetKey: "TinyHouseRace03_0001",
        trackKey: "03-BasementDwellers_0001",
      },
      {
        name: "H01 - Floor Buffer",
        assetKey: "TinyHouseHoverdroneRace01_0001",
        trackKey: "H01-FloorBuffer_0001",
      },
    ],
  },
  {
    environmentId: "TinyHouse_Night",
    name: "San Lipo Drive - Curfew",
    races: [
      {
        name: "01 - Peace Talks",
        assetKey: "TinyHouseNightRace01_0001",
        trackKey: "01-PeaceTalks_0001",
      },
      {
        name: "02 - Starry Night",
        assetKey: "TinyHouseNightRace02_0001",
        trackKey: "02-StarryNight_0001",
      },
      {
        name: "03 - Game Room",
        assetKey: "TinyHouseNightRace03_0001",
        trackKey: "03-GameRoom_0001",
      },
      {
        name: "H01 - Table Tennis",
        assetKey: "TinyHouseNightHoverdroneRace01_0001",
        trackKey: "H01-TableTennis_0001",
      },
    ],
  },
  {
    environmentId: "TinyHouse_Empty",
    name: "San Lipo Drive - Empty",
    races: [],
  },
];

export const MICRO_DRONES_LEVELS: string[] = MICRO_DRONES_LEVEL_DATA.map(
  (level) => level.name,
);

export const MICRO_DRONES_RACES: string[] = Array.from(
  new Set(
    MICRO_DRONES_LEVEL_DATA.flatMap((level) =>
      level.races.map((race) => race.name),
    ),
  ),
);

export const MICRO_DRONES_RACES_BY_LEVEL: Record<string, string[]> =
  Object.fromEntries(
    MICRO_DRONES_LEVEL_DATA.map((level) => [
      level.name,
      level.races.map((race) => race.name),
    ]),
  );

/**
 * Liftoff: FPV Drone Racing shipped environments and official race tracks.
 * Extracted from local Unity metadata only: Environment ScriptableObject display
 * names plus bundled Race/Track XML names. No game assets are bundled here.
 */
export const FPV_LEVEL_DATA: LiftoffLevelSeed[] = [
  {
    environmentId: "StrawBale",
    name: "Straw Bale",
    races: [
      {
        name: "01 - Field Day",
        assetKey: "StrawBaleRace01_0001",
        trackKey: "StrawBaleTrack01_0001",
      },
      {
        name: "02 - After Hours",
        assetKey: "StrawBaleRace02_0001",
        trackKey: "StrawBaleTrack02_0001",
      },
      {
        name: "03 - Loop The Silo",
        assetKey: "StrawBaleRace03_0001",
        trackKey: "StrawBaleTrack03_0001",
      },
      {
        name: "04 - Against The Grain",
        assetKey: "StrawBaleRace04_0001",
        trackKey: "StrawBaleTrack04_0001",
      },
      {
        name: "05 - Barn Burner",
        assetKey: "StrawBaleRace05_0001",
        trackKey: "StrawBaleTrack05_0001",
      },
      {
        name: "06 - Further Afield",
        assetKey: "StrawBaleRace06_0001",
        trackKey: "StrawBaleTrack06_0001",
      },
    ],
  },
  {
    environmentId: "PineValley",
    name: "Pine Valley",
    races: [
      {
        name: "01 - Forest For The Trees",
        assetKey: "PineValleyRace01_0001",
        trackKey: "PineValleyTrack01_0001",
      },
      {
        name: "02 - The Great Outdoors",
        assetKey: "PineValleyRace02_0001",
        trackKey: "PineValleyTrack02_0001",
      },
      {
        name: "03 - Rock And Roll",
        assetKey: "PineValleyRace03_0001",
        trackKey: "PineValleyTrack03_0001",
      },
      {
        name: "04 - Wildcamping",
        assetKey: "PineValleyRace04_0001",
        trackKey: "PineValleyTrack04_0001",
      },
      {
        name: "05 - Timber",
        assetKey: "PineValleyRace05_0001",
        trackKey: "PineValleyTrack05_0001",
      },
    ],
  },
  {
    environmentId: "MinusTwo",
    name: "Minus Two",
    races: [
      {
        name: "01 - Turn Signals",
        assetKey: "MinusTwoRace01_0001",
        trackKey: "MinusTwoTrack01_0001",
      },
      {
        name: "02 - Saro's Revenge",
        assetKey: "MinusTwoRace02_0001",
        trackKey: "MinusTwoTrack02_0001",
      },
      {
        name: "03 - The Underground Scene",
        assetKey: "MinusTwoRace03_0001",
        trackKey: "MinusTwoTrack03_0001",
      },
      {
        name: "04 - Concrete Jungle",
        assetKey: "MinusTwoRace04_0001",
        trackKey: "MinusTwoTrack04_0001",
      },
      {
        name: "05 - Drift Style",
        assetKey: "MinusTwoRace05_0001",
        trackKey: "MinusTwoTrack05_0001",
      },
    ],
  },
  {
    environmentId: "AutumnFields",
    name: "Autumn Fields",
    races: [
      {
        name: "01 - Walk In The Park",
        assetKey: "AutumnFieldsRace01_0001",
        trackKey: "AutumnFieldsTrack01_0001",
      },
      {
        name: "02 - Sweater Weather",
        assetKey: "AutumnFieldsRace02_0001",
        trackKey: "AutumnFieldsTrack02_0001",
      },
      {
        name: "03 - Stick Time",
        assetKey: "AutumnFieldsRace03_0001",
        trackKey: "AutumnFieldsTrack03_0001",
      },
      {
        name: "04 - Mudlarking",
        assetKey: "AutumnFieldsRace04_0001",
        trackKey: "AutumnFieldsTrack04_0001",
      },
      {
        name: "05 - A League Of Its Own",
        assetKey: "AutumnFieldsRace05_0001",
        trackKey: "AutumnFieldsTrack05_0001",
      },
      {
        name: "06 - Autumn Airspace",
        assetKey: "AutumnFieldsRace06_0001",
        trackKey: "AutumnFieldsTrack06_0001",
      },
    ],
  },
  {
    environmentId: "HangarC03",
    name: "Hangar C03",
    races: [
      {
        name: "01 - Shipments",
        assetKey: "HangarC03Race01_0001",
        trackKey: "HangarC03Track01_0001",
      },
      {
        name: "02 - Parcel Tracking",
        assetKey: "HangarC03Race02_0001",
        trackKey: "HangarC03Track02_0001",
      },
    ],
  },
  {
    environmentId: "LiftoffArena",
    name: "Liftoff Arena",
    races: [
      {
        name: "01 - Mexican Wave",
        assetKey: "LiftoffArenaRace01_0001",
        trackKey: "LiftoffArenaTrack01_0001",
      },
      {
        name: "02 - Swing For The Bleachers",
        assetKey: "LiftoffArenaRace02_0001",
        trackKey: "LiftoffArenaTrack02_0001",
      },
      {
        name: "03 - Grandstand",
        assetKey: "LiftoffArenaRace03_0001",
        trackKey: "LiftoffArenaTrack03_0001",
      },
      {
        name: "04 - Infinity Loop",
        assetKey: "LiftoffArenaRace04_0001",
        trackKey: "LiftoffArenaTrack04_0001",
      },
      {
        name: "05 - In The Spotlight",
        assetKey: "LiftoffArenaRace05_0001",
        trackKey: "LiftoffArenaTrack05_0001",
      },
      {
        name: "06 - Touchdown",
        assetKey: "LiftoffArenaRace06_0001",
        trackKey: "LiftoffArenaTrack06_0001",
      },
      {
        name: "07 - Round and Around",
        assetKey: "LiftoffArenaRace07_0001",
        trackKey: "LiftoffArenaTrack07_0001",
      },
    ],
  },
  {
    environmentId: "DubaiLegends",
    name: "Dubai Legends",
    races: [
      {
        name: "01 - Legendary Night",
        assetKey: "DubaiLegendsRace01_0001",
        trackKey: "DubaiLegendsTrack01_0001",
      },
    ],
  },
  {
    environmentId: "Hannover",
    name: "Hannover",
    races: [
      {
        name: "01 - The Biggest Yet",
        assetKey: "HannoverRace01_0001",
        trackKey: "HannoverTrack01_0001",
      },
      {
        name: "02 - Bring Me A Shrubbery",
        assetKey: "HannoverRace02_0001",
        trackKey: "HannoverTrack02_0001",
      },
      {
        name: "03 - Cone Off",
        assetKey: "HannoverRace03_0001",
        trackKey: "HannoverTrack03_0001",
      },
      {
        name: "04 - Around The Block",
        assetKey: "HannoverRace04_0001",
        trackKey: "HannoverTrack04_0001",
      },
      {
        name: "05 - Got Intel",
        assetKey: "HannoverRace05_0001",
        trackKey: "HannoverTrack05_0001",
      },
    ],
  },
  {
    environmentId: "ParisDroneFestival",
    name: "Paris Drone Festival",
    races: [
      {
        name: "01 - City Trip",
        assetKey: "ParisDroneFestivalRace01_0001",
        trackKey: "ParisDroneFestivalTrack01_0001",
      },
      {
        name: "02 - Triumph",
        assetKey: "ParisDroneFestivalRace02_0001",
        trackKey: "ParisDroneFestivalTrack02_0001",
      },
      {
        name: "03 - City Of Lights",
        assetKey: "ParisDroneFestivalRace03_0001",
        trackKey: "ParisDroneFestivalTrack03_0001",
      },
      {
        name: "04 - Promenading",
        assetKey: "ParisDroneFestivalRace04_0001",
        trackKey: "ParisDroneFestivalTrack04_0001",
      },
      {
        name: "05 - Stage Fright",
        assetKey: "ParisDroneFestivalRace05_0001",
        trackKey: "ParisDroneFestivalTrack05_0001",
      },
    ],
  },
  {
    environmentId: "ThePit",
    name: "The Pit",
    races: [
      {
        name: "01 - Way Down In The Hole",
        assetKey: "ThePitRace01_0001",
        trackKey: "ThePitTrack01_0001",
      },
      {
        name: "02 - Into The Abyss",
        assetKey: "ThePitRace02_0001",
        trackKey: "ThePitTrack02_0001",
      },
      {
        name: "03 - Jackpot",
        assetKey: "ThePitRace03_0001",
        trackKey: "ThePitTrack03_0001",
      },
      {
        name: "04 - The Red Baron",
        assetKey: "ThePitRace04_0001",
        trackKey: "ThePitTrack04_0001",
      },
      {
        name: "05 - Reservoir",
        assetKey: "ThePitRace05_0001",
        trackKey: "ThePitTrack05_0001",
      },
      {
        name: "06 - Conveyor Belt Dive",
        assetKey: "ThePitRace06_0001",
        trackKey: "ThePitTrack06_0001",
      },
    ],
  },
  {
    environmentId: "TheGreen",
    name: "The Green",
    races: [
      {
        name: "01 - Par For The Course",
        assetKey: "TheGreenRace01_0001",
        trackKey: "TheGreenTrack01_0001",
      },
      {
        name: "02 - Rolling Hills",
        assetKey: "TheGreenRace02_0001",
        trackKey: "TheGreenTrack02_0001",
      },
      {
        name: "03 - Club House",
        assetKey: "TheGreenRace03_0001",
        trackKey: "TheGreenTrack03_0001",
      },
      {
        name: "04 - Flop Shot",
        assetKey: "TheGreenRace04_0001",
        trackKey: "TheGreenTrack04_0001",
      },
      {
        name: "05 - Tee Off",
        assetKey: "TheGreenRace05_0001",
        trackKey: "TheGreenTrack05_0001",
      },
      {
        name: "06 - The Nineteenth Hole",
        assetKey: "TheGreenRace06_0001",
        trackKey: "TheGreenTrack06_0001",
      },
    ],
  },
  {
    environmentId: "Hall26",
    name: "Hall 26",
    races: [
      {
        name: "01 - Race Around The Rafters",
        assetKey: "Hall26Race01_0001",
        trackKey: "Hall26Track01_0001",
      },
      {
        name: "02 - Hula Hoop",
        assetKey: "Hall26Race02_0001",
        trackKey: "Hall26Track02_0001",
      },
      {
        name: "03 - Hall Of Fame",
        assetKey: "Hall26Race03_0001",
        trackKey: "Hall26Track03_0001",
      },
      {
        name: "04 - A Roof Over Your Head",
        assetKey: "Hall26Race04_0001",
        trackKey: "Hall26Track04_0001",
      },
      {
        name: "05 - Support Structure",
        assetKey: "Hall26Race05_0001",
        trackKey: "Hall26Track05_0001",
      },
    ],
  },
  {
    environmentId: "BardwellsYard",
    name: "Bardwell's Yard",
    races: [
      {
        name: "01 - Front Porch Frenzy",
        assetKey: "BardwellsYardRace01_0001",
        trackKey: "BardwellsYardTrack01_0001",
      },
      {
        name: "02 - Learn Something Today",
        assetKey: "BardwellsYardRace02_0001",
        trackKey: "BardwellsYardTrack02_0001",
      },
      {
        name: "03 - Stuff That Works",
        assetKey: "BardwellsYardRace03_0001",
        trackKey: "BardwellsYardTrack03_0001",
      },
      {
        name: "04 - Know It All",
        assetKey: "BardwellsYardRace04_0001",
        trackKey: "BardwellsYardTrack04_0001",
      },
      {
        name: "05 - Humble Beginnings",
        assetKey: "BardwellsYardRace05_0001",
        trackKey: "BardwellsYardTrack05_0001",
      },
      {
        name: "06 - Birdhouse",
        assetKey: "BardwellsYardRace06_0001",
        trackKey: "BardwellsYardTrack06_0001",
      },
    ],
  },
  {
    environmentId: "BandoCity",
    name: "Bando City",
    races: [
      {
        name: "01 - Spare Tires",
        assetKey: "BandoCityRace01_0001",
        trackKey: "BandoCityTrack01_0001",
      },
      {
        name: "02 - Who Needs Stairs",
        assetKey: "BandoCityRace02_0001",
        trackKey: "BandoCityTrack02_0001",
      },
      {
        name: "03 - Pipe Dream",
        assetKey: "BandoCityRace03_0001",
        trackKey: "BandoCityTrack03_0001",
      },
      {
        name: "04 - Choice Matters",
        assetKey: "BandoCityRace04_0001",
        trackKey: "BandoCityTrack04_0001",
      },
      {
        name: "05 - Under Construction",
        assetKey: "BandoCityRace05_0001",
        trackKey: "BandoCityTrack05_0001",
      },
    ],
  },
  {
    environmentId: "TheRussianWoodpecker",
    name: "The Woodpecker",
    races: [
      {
        name: "01 - Over The Horizon",
        assetKey: "TheRussianWoodpeckerRace01_0001",
        trackKey: "TheRussianWoodpeckerTrack01_0001",
      },
      {
        name: "02 - Steel Yard",
        assetKey: "TheRussianWoodpeckerRace02_0001",
        trackKey: "TheRussianWoodpeckerTrack02_0001",
      },
    ],
  },
  {
    environmentId: "ShortCircuit",
    name: "Short Circuit",
    races: [
      {
        name: "01 - Pole Position",
        assetKey: "ShortCircuitRace01_0001",
        trackKey: "ShortCircuitTrack01_0001",
      },
      {
        name: "02 - Diskart",
        assetKey: "ShortCircuitRace02_0001",
        trackKey: "ShortCircuitTrack02_0001",
      },
      {
        name: "03 - Overpass",
        assetKey: "ShortCircuitRace03_0001",
        trackKey: "ShortCircuitTrack03_0001",
      },
    ],
  },
  {
    environmentId: "Surtur",
    name: "Surtur",
    races: [
      {
        name: "01 - Hiking Trail",
        assetKey: "SurturRace01_0001",
        trackKey: "SurturTrack01_0001",
      },
      {
        name: "02 - Mountaineering",
        assetKey: "SurturRace02_0001",
        trackKey: "SurturTrack02_0001",
      },
      {
        name: "03 - Surtalogi Tour",
        assetKey: "SurturRace03_0001",
        trackKey: "SurturTrack03_0001",
      },
      {
        name: "04 - The Floor is Lava",
        assetKey: "SurturRace04_0001",
        trackKey: "SurturTrack04_0001",
      },
    ],
  },
  {
    environmentId: "Permafrost",
    name: "Permafrost",
    races: [
      {
        name: "01 - Under Pressure",
        assetKey: "PermafrostRace01_0001",
        trackKey: "PermafrostTrack01_0001",
      },
      {
        name: "02 - Mayday",
        assetKey: "PermafrostRace02_0001",
        trackKey: "PermafrostTrack02_0001",
      },
      {
        name: "03 - Slalom",
        assetKey: "PermafrostRace03_0001",
        trackKey: "PermafrostTrack03_0001",
      },
    ],
  },
  {
    environmentId: "Rustline",
    name: "Rustline",
    races: [
      {
        name: "01 - Railline",
        assetKey: "RustlineRace01_0001",
        trackKey: "RustlineTrack01_0001",
      },
      {
        name: "02 - Factoryline",
        assetKey: "RustlineRace02_0001",
        trackKey: "RustlineTrack02_0001",
      },
      {
        name: "03 - Pipeline",
        assetKey: "RustlineRace03_0001",
        trackKey: "RustlineTrack03_0001",
      },
    ],
  },
  {
    environmentId: "TheDrawingBoard",
    name: "The Drawing Board",
    races: [],
  },
];

export const FPV_LEVELS: string[] = FPV_LEVEL_DATA.map((level) => level.name);

export const FPV_RACES: string[] = Array.from(
  new Set(FPV_LEVEL_DATA.flatMap((level) => level.races.map((race) => race.name))),
);

export const FPV_RACES_BY_LEVEL: Record<string, string[]> =
  Object.fromEntries(
    FPV_LEVEL_DATA.map((level) => [
      level.name,
      level.races.map((race) => race.name),
    ]),
  );

export const LIFTOFF_LEVELS: string[] = [
  ...MICRO_DRONES_LEVELS,
  ...FPV_LEVELS,
];

export const LIFTOFF_RACES: string[] = Array.from(
  new Set([
    ...MICRO_DRONES_RACES,
    ...FPV_RACES,
  ]),
);

export const LIFTOFF_RACES_BY_LEVEL: Record<string, string[]> = {
  ...MICRO_DRONES_RACES_BY_LEVEL,
  ...FPV_RACES_BY_LEVEL,
};
