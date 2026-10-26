# Script and console commands

The command set the game's script compiler and console know, as they appear in the
retail 1.2.0416 executable. Opcodes matter because compiled scripts (`SCDA` in `SCPT`,
and result scripts in `INFO`/`QUST`/`PACK`) store a command as its opcode, not its name,
so an engine running the game's own compiled scripts must use exactly these numbers.

**Source:** read from the executable's two command tables (their location comes from
the public xOBSE source). Help strings were left out. **Confidence:** confirmed for
names, opcodes, the needs-a-reference flag and parameter kinds.

## How the table works

- Script commands are numbered from `0x1000` upwards with no gaps (369 entries, the
  last `0x1170`). Console-only commands are numbered from `0x0100` (131 entries).
  Script extenders start their own commands at `0x1400`, so anything from `0x1171` to
  `0x13FF` is unused in vanilla.
- **Ref** means the command runs on a reference: `player.GetPos z` or, inside an
  object script, on the scripted object itself.
- **Parameters** lists the kind of each argument; `[...]` marks an optional one. The
  kind decides how the compiler encodes the argument (a form reference becomes an index
  into the script's reference list, an axis becomes the character `X`/`Y`/`Z`, an
  actor value and an anim group become 16-bit codes, a variable name is resolved
  against the target's script).
- A few commands parse their arguments specially (for example `MessageBox` and
  `Message` take a format string followed by a variable number of values, more than
  the single declared parameter suggests).
- Kinds by numeric id, in order from 0: string, int, float, inventory object,
  reference, actor value, actor, spell, axis, cell, anim group, magic item, sound,
  topic, quest, race, class, faction, sex, global, furniture, object, variable name,
  quest stage, map marker, actor base, container, worldspace, crime type, package,
  combat style, magic effect, birthsign, form type, weather, NPC, owner, effect shader.

## Script commands

| Opcode | Name | Short | Ref | Parameters |
|---|---|---|---|---|
| 0x1000 | MessageBox |  |  | string |
| 0x1001 | GetDistance |  | yes | reference |
| 0x1002 | AddItem |  | yes | inventory object, int |
| 0x1003 | SetEssential |  |  | actor base, [int] |
| 0x1004 | Rotate |  | yes | axis, int |
| 0x1005 | GetLocked |  | yes |  |
| 0x1006 | GetPos |  | yes | axis |
| 0x1007 | SetPos |  | yes | axis, float |
| 0x1008 | GetAngle |  | yes | axis |
| 0x1009 | SetAngle |  | yes | axis, float |
| 0x100A | GetStartingPos |  | yes | axis |
| 0x100B | GetStartingAngle |  | yes | axis |
| 0x100C | GetSecondsPassed |  |  |  |
| 0x100D | Activate |  | yes | [reference], [int] |
| 0x100E | GetActorValue | GetAV | yes | actor value |
| 0x100F | SetActorValue | SetAV | yes | actor value, int |
| 0x1010 | ModActorValue | ModAV | yes | actor value, int |
| 0x1011 | SetAtStart |  | yes |  |
| 0x1012 | GetCurrentTime |  |  |  |
| 0x1013 | PlayGroup |  | yes | anim group, int |
| 0x1014 | LoopGroup |  | yes | anim group, int, int |
| 0x1015 | SkipAnim |  | yes |  |
| 0x1016 | StartCombat |  | yes | actor |
| 0x1017 | StopCombat |  | yes |  |
| 0x1018 | GetScale |  | yes |  |
| 0x1019 | UnusedFunction1 |  | yes |  |
| 0x101A | UnusedFunction2 |  | yes |  |
| 0x101B | GetLineOfSight | GetLOS | yes | reference |
| 0x101C | AddSpell |  | yes | spell |
| 0x101D | RemoveSpell |  | yes | spell |
| 0x101E | Cast |  | yes | magic item, reference |
| 0x101F | GetButtonPressed |  |  |  |
| 0x1020 | GetInSameCell |  | yes | reference |
| 0x1021 | Enable |  | yes |  |
| 0x1022 | Disable |  | yes |  |
| 0x1023 | GetDisabled |  | yes |  |
| 0x1024 | MenuMode |  |  | [int] |
| 0x1025 | PlaceAtMe |  | yes | object, [int], [int], [int] |
| 0x1026 | PlaySound |  |  | sound |
| 0x1027 | GetDisease |  | yes |  |
| 0x1028 | GetVampire |  | yes |  |
| 0x1029 | GetClothingValue |  | yes |  |
| 0x102A | SameFaction |  | yes | actor |
| 0x102B | SameRace |  | yes | actor |
| 0x102C | SameSex |  | yes | actor |
| 0x102D | GetDetected |  | yes | actor |
| 0x102E | GetDead |  | yes |  |
| 0x102F | GetItemCount |  | yes | inventory object |
| 0x1030 | GetGold |  | yes |  |
| 0x1031 | GetSleeping |  | yes |  |
| 0x1032 | GetTalkedToPC |  | yes |  |
| 0x1033 | Say |  | yes | topic, [int], [NPC], [int], [int] |
| 0x1034 | SayTo |  | yes | actor, topic, [int] |
| 0x1035 | GetScriptVariable |  |  | reference, variable name |
| 0x1036 | StartQuest |  |  | quest |
| 0x1037 | StopQuest |  |  | quest |
| 0x1038 | GetQuestRunning | GetQR |  | quest |
| 0x1039 | SetStage |  |  | quest, quest stage |
| 0x103A | GetStage |  |  | quest |
| 0x103B | GetStageDone |  |  | quest, quest stage |
| 0x103C | GetFactionRankDifference |  | yes | faction, actor |
| 0x103D | GetAlarmed |  | yes |  |
| 0x103E | IsRaining |  |  |  |
| 0x103F | GetAttacked |  | yes |  |
| 0x1040 | GetIsCreature |  | yes |  |
| 0x1041 | GetLockLevel |  | yes |  |
| 0x1042 | GetShouldAttack |  | yes | actor |
| 0x1043 | GetInCell |  | yes | cell |
| 0x1044 | GetIsClass |  | yes | class |
| 0x1045 | GetIsRace |  | yes | race |
| 0x1046 | GetIsSex |  | yes | sex |
| 0x1047 | GetInFaction |  | yes | faction |
| 0x1048 | GetIsID |  | yes | object |
| 0x1049 | GetFactionRank |  | yes | faction |
| 0x104A | GetGlobalValue |  |  | global |
| 0x104B | IsSnowing |  |  |  |
| 0x104C | GetDisposition |  | yes | actor |
| 0x104D | GetRandomPercent |  |  |  |
| 0x104E | StreamMusic |  |  | string |
| 0x104F | GetQuestVariable |  |  | quest, variable name |
| 0x1050 | GetLevel |  | yes |  |
| 0x1051 | GetArmorRating |  | yes |  |
| 0x1052 | RemoveItem |  | yes | inventory object, int |
| 0x1053 | ModDisposition |  | yes | actor, int |
| 0x1054 | GetDeadCount |  |  | actor base |
| 0x1055 | ShowMap |  |  | map marker, [int] |
| 0x1056 | StartConversation |  | yes | actor, [topic] |
| 0x1057 | Drop |  | yes | inventory object, int |
| 0x1058 | AddTopic |  |  | topic |
| 0x1059 | Message |  |  | string |
| 0x105A | SetAlert |  | yes | int |
| 0x105B | GetIsAlerted |  | yes |  |
| 0x105C | Look |  | yes | reference |
| 0x105D | StopLook |  | yes |  |
| 0x105E | EvaluatePackage | evp | yes |  |
| 0x105F | Yield |  | yes |  |
| 0x1060 | EnablePlayerControls |  |  |  |
| 0x1061 | DisablePlayerControls |  |  |  |
| 0x1062 | GetPlayerControlsDisabled |  |  |  |
| 0x1063 | GetHeadingAngle |  | yes | reference |
| 0x1064 | PickIdle |  | yes |  |
| 0x1065 | IsWeaponOut |  | yes |  |
| 0x1066 | IsTorchOut |  | yes |  |
| 0x1067 | IsShieldOut |  | yes |  |
| 0x1068 | IsYielding |  | yes |  |
| 0x1069 | IsActionRef |  | yes | reference |
| 0x106A | IsFacingUp |  | yes |  |
| 0x106B | GetKnockedState |  | yes |  |
| 0x106C | GetWeaponAnimType |  | yes |  |
| 0x106D | GetWeaponSkillType |  | yes |  |
| 0x106E | GetCurrentAIPackage |  | yes |  |
| 0x106F | IsWaiting |  | yes |  |
| 0x1070 | IsIdlePlaying |  | yes |  |
| 0x1071 | CompleteQuest |  |  | quest |
| 0x1072 | Lock |  | yes | [int], [int] |
| 0x1073 | UnLock |  | yes | [int] |
| 0x1074 | GetCrimeGold |  | yes |  |
| 0x1075 | SetCrimeGold |  | yes | int |
| 0x1076 | ModCrimeGold |  | yes | int |
| 0x1077 | GetCrimeKnown |  | yes | crime type, actor, actor |
| 0x1078 | PositionWorld | PosWorld | yes | float, float, float, float, worldspace |
| 0x1079 | PositionCell | PosCell | yes | float, float, float, float, cell |
| 0x107A | GetCrime |  | yes | actor, [crime type] |
| 0x107B | PayFine |  | yes |  |
| 0x107C | PayFineThief |  | yes |  |
| 0x107D | IsGuard |  | yes |  |
| 0x107E | GoToJail |  | yes |  |
| 0x107F | CanPayCrimeGold |  | yes |  |
| 0x1080 | GetFatiguePercentage | GetFatigue | yes |  |
| 0x1081 | GetPCIsClass |  |  | class |
| 0x1082 | GetPCIsRace |  |  | race |
| 0x1083 | GetPCIsSex |  |  | sex |
| 0x1084 | GetPCInFaction |  |  | faction |
| 0x1085 | SameFactionAsPC |  | yes |  |
| 0x1086 | SameRaceAsPC |  | yes |  |
| 0x1087 | SameSexAsPC |  | yes |  |
| 0x1088 | GetIsReference |  | yes | reference |
| 0x1089 | SetFactionRank |  | yes | faction, int |
| 0x108A | ModFactionRank |  | yes | faction, int |
| 0x108B | KillActor | kill | yes | [actor] |
| 0x108C | ResurrectActor | resurrect | yes | [int] |
| 0x108D | IsTalking |  | yes |  |
| 0x108E | GetWalkSpeed | GetWalk | yes |  |
| 0x108F | GetCurrentAIProcedure |  | yes |  |
| 0x1090 | GetTrespassWarningLevel |  | yes |  |
| 0x1091 | IsTrespassing |  | yes |  |
| 0x1092 | IsInMyOwnedCell |  | yes |  |
| 0x1093 | GetWindSpeed |  |  |  |
| 0x1094 | GetCurrentWeatherPercent | getweatherpct |  |  |
| 0x1095 | GetIsCurrentWeather | getweather |  | weather |
| 0x1096 | IsContinuingPackagePCNear |  | yes |  |
| 0x1097 | AddScriptPackage |  | yes | package |
| 0x1098 | RemoveScriptPackage |  | yes |  |
| 0x1099 | CanHaveFlames |  | yes |  |
| 0x109A | HasFlames |  | yes |  |
| 0x109B | AddFlames |  | yes |  |
| 0x109C | RemoveFlames |  | yes |  |
| 0x109D | GetOpenState |  | yes |  |
| 0x109E | MoveToMarker | MoveTo | yes | reference, [float], [float], [float] |
| 0x109F | GetSitting |  | yes |  |
| 0x10A0 | GetFurnitureMarkerID |  | yes |  |
| 0x10A1 | GetIsCurrentPackage |  | yes | package |
| 0x10A2 | IsCurrentFurnitureRef |  | yes | reference |
| 0x10A3 | IsCurrentFurnitureObj |  | yes | furniture |
| 0x10A4 | SetSize | CSize | yes | float |
| 0x10A5 | RemoveMe |  | yes | [container] |
| 0x10A6 | DropMe |  | yes |  |
| 0x10A7 | GetFactionReaction |  |  | faction, faction |
| 0x10A8 | SetFactionReaction |  |  | faction, faction, int |
| 0x10A9 | ModFactionReaction |  |  | faction, faction, int |
| 0x10AA | GetDayOfWeek |  |  |  |
| 0x10AB | IsPlayerInJail |  |  |  |
| 0x10AC | GetTalkedToPCParam |  |  | actor |
| 0x10AD | RemoveAllItems |  | yes | [container], [int] |
| 0x10AE | WakeUpPC |  |  | [int] |
| 0x10AF | IsPCSleeping |  |  |  |
| 0x10B0 | IsPCAMurderer |  |  |  |
| 0x10B1 | SetCombatStyle | setcs | yes | [combat style] |
| 0x10B2 | PlaySound3D |  |  | sound |
| 0x10B3 | SelectPlayerSpell | spspell |  | spell |
| 0x10B4 | GetDetectionLevel |  | yes | actor |
| 0x10B5 | IsActorDetected |  | yes |  |
| 0x10B6 | GetEquipped |  | yes | inventory object |
| 0x10B7 | Wait |  | yes | package |
| 0x10B8 | StopWaiting |  | yes | package |
| 0x10B9 | IsSwimming |  | yes |  |
| 0x10BA | ScriptEffectElapsedSeconds |  |  |  |
| 0x10BB | SetCellPublicFlag | setpublic |  | cell, int |
| 0x10BC | GetPCSleepHours |  |  |  |
| 0x10BD | SetPCSleepHours |  |  | int |
| 0x10BE | GetAmountSoldStolen |  |  |  |
| 0x10BF | ModAmountSoldStolen |  |  | int |
| 0x10C0 | CloseCurrentOblivionGate |  |  | [int] |
| 0x10C1 | GetPCExpelled |  |  | faction |
| 0x10C2 | SetPCExpelled |  |  | faction, int |
| 0x10C3 | GetPCFactionMurder |  |  | faction |
| 0x10C4 | SetPCFactionMurder |  |  | faction, int |
| 0x10C5 | GetPCFactionSteal |  |  | faction |
| 0x10C6 | SetPCFactionSteal |  |  | faction, int |
| 0x10C7 | GetPCFactionAttack |  |  | faction |
| 0x10C8 | SetPCFactionAttack |  |  | faction, int |
| 0x10C9 | GetPCFactionSubmitAuthority |  |  | faction |
| 0x10CA | SetPCFactionSubmitAuthority |  |  | faction, int |
| 0x10CB | GetDestroyed |  | yes |  |
| 0x10CC | SetDestroyed |  | yes | int |
| 0x10CD | GetActionRef | getAR | yes |  |
| 0x10CE | GetSelf | this | yes |  |
| 0x10CF | GetContainer |  | yes |  |
| 0x10D0 | GetForceRun |  | yes |  |
| 0x10D1 | SetForceRun |  | yes | int |
| 0x10D2 | GetForceSneak |  | yes |  |
| 0x10D3 | SetForceSneak |  | yes | int |
| 0x10D4 | AdvancePCSkill | AdvSkill |  | actor value, int |
| 0x10D5 | AdvancePCLevel | AdvLevel |  |  |
| 0x10D6 | HasMagicEffect |  | yes | magic effect |
| 0x10D7 | GetDoorDefaultOpen |  | yes |  |
| 0x10D8 | SetDoorDefaultOpen |  | yes | int |
| 0x10D9 | ShowClassMenu |  |  |  |
| 0x10DA | ShowRaceMenu |  |  |  |
| 0x10DB | ShowBirthSignMenu |  |  |  |
| 0x10DC | ShowNameMenu |  |  |  |
| 0x10DD | SetOpenState |  | yes | int |
| 0x10DE | CloseOblivionGate |  | yes | [int] |
| 0x10DF | IsSpellTarget |  | yes | magic item |
| 0x10E0 | GetIsPlayerBirthsign | GetPBS |  | birthsign |
| 0x10E1 | GetPersuasionNumber |  |  |  |
| 0x10E2 | VampireFeed |  | yes | actor |
| 0x10E3 | HasVampireFed |  | yes |  |
| 0x10E4 | GetIsClassDefault |  |  | class |
| 0x10E5 | GetClassDefaultMatch |  |  |  |
| 0x10E6 | GetInCellParam |  |  | cell, reference |
| 0x10E7 | SetInChargen |  |  | int |
| 0x10E8 | GetCombatTarget |  | yes |  |
| 0x10E9 | GetPackageTarget |  | yes |  |
| 0x10EA | ShowSpellMaking |  |  |  |
| 0x10EB | ShowEnchantment |  |  |  |
| 0x10EC | SetGhost |  | yes | int |
| 0x10ED | GetIsGhost |  | yes |  |
| 0x10EE | EquipItem | EquipObject | yes | inventory object, [int] |
| 0x10EF | UnequipItem | UnEquipObject | yes | inventory object, [int] |
| 0x10F0 | SetClass |  | yes | class |
| 0x10F1 | SetUnconscious |  |  | int |
| 0x10F2 | GetUnconscious |  | yes |  |
| 0x10F3 | SetRestrained |  |  | int |
| 0x10F4 | GetRestrained |  | yes |  |
| 0x10F5 | ForceFlee | Flee | yes | [cell], [reference] |
| 0x10F6 | GetIsUsedItem |  |  | object |
| 0x10F7 | GetIsUsedItemType |  |  | form type |
| 0x10F8 | ModPCFame |  |  | int |
| 0x10F9 | GetPCFame |  |  |  |
| 0x10FA | ModPCInfamy |  |  | int |
| 0x10FB | GetPCInfamy |  |  |  |
| 0x10FC | SetPCFame |  |  | int |
| 0x10FD | SetPCInfamy |  |  | int |
| 0x10FE | GetIsPlayableRace |  | yes |  |
| 0x10FF | GetOffersServicesNow |  | yes |  |
| 0x1100 | GetGameSetting | GetGS |  | string |
| 0x1101 | StopCombatAlarmOnActor | SCAOnActor | yes |  |
| 0x1102 | GetUsedItemLevel |  |  |  |
| 0x1103 | GetUsedItemActivate |  |  |  |
| 0x1104 | SetWeather | sw |  | weather, [int] |
| 0x1105 | ForceTakeCover | TakeCover | yes | actor, int |
| 0x1106 | ModBarterGold |  | yes | int |
| 0x1107 | SetBarterGold |  | yes | int |
| 0x1108 | GetBarterGold |  | yes |  |
| 0x1109 | IsTimePassing |  |  |  |
| 0x110A | IsPleasant |  |  |  |
| 0x110B | IsCloudy |  |  |  |
| 0x110C | TrapUpdate |  |  |  |
| 0x110D | SetQuestObject |  |  | object, int |
| 0x110E | ForceActorValue | ForceAV | yes | actor value, int |
| 0x110F | ModPCSkill | ModPCS |  | actor value, int |
| 0x1110 | ModPCAttribute | ModPCA |  | actor value, int |
| 0x1111 | EnableFastTravel | EnableFast |  | int |
| 0x1112 | GetArmorRatingUpperBody |  | yes |  |
| 0x1113 | GetParentRef |  | yes |  |
| 0x1114 | PlayBink |  |  | string, [int] |
| 0x1115 | GetBaseActorValue | GetBaseAV | yes | actor value |
| 0x1116 | IsOwner |  | yes | [owner] |
| 0x1117 | SetOwnership |  | yes | [owner] |
| 0x1118 | IsCellOwner |  | yes | cell, [owner] |
| 0x1119 | SetCellOwnership |  |  | cell, [owner] |
| 0x111A | IsHorseStolen |  | yes |  |
| 0x111B | SetCellFullName |  |  | cell, string |
| 0x111C | SetActorFullName |  | yes | string |
| 0x111D | IsLeftUp |  | yes |  |
| 0x111E | IsSneaking |  | yes |  |
| 0x111F | IsRunning |  | yes |  |
| 0x1120 | GetFriendHit |  | yes | actor |
| 0x1121 | IsInCombat |  | yes |  |
| 0x1122 | SetPackDuration | SPDur | yes | float |
| 0x1123 | PlayMagicShaderVisuals | PMS | yes | effect shader, [float] |
| 0x1124 | PlayMagicEffectVisuals | PME | yes | magic effect, [float] |
| 0x1125 | StopMagicShaderVisuals | SMS | yes | effect shader |
| 0x1126 | StopMagicEffectVisuals | SME | yes | magic effect |
| 0x1127 | ResetInterior |  |  | cell |
| 0x1128 | IsAnimPlaying |  | yes |  |
| 0x1129 | SetActorAlpha | SAA | yes | float |
| 0x112A | EnableLinkedPathPoints |  | yes |  |
| 0x112B | DisableLinkedPathPoints |  | yes |  |
| 0x112C | IsInInterior |  | yes |  |
| 0x112D | ForceWeather | fw |  | weather, [int] |
| 0x112E | ToggleActorsAI |  | yes |  |
| 0x112F | IsActorsAIOff |  | yes |  |
| 0x1130 | SetInvestmentGold |  | yes | int |
| 0x1131 | GetInvestmentGold |  | yes |  |
| 0x1132 | IsActorUsingATorch |  | yes |  |
| 0x1133 | SetLevel |  | yes | int, [int], [int], [int] |
| 0x1134 | ResetFallDamageTimer |  | yes |  |
| 0x1135 | IsXBox |  |  |  |
| 0x1136 | GetInWorldspace |  | yes | worldspace |
| 0x1137 | ModPCMiscStat | ModPCMS |  | int, int |
| 0x1138 | GetPCMiscStat | GetPCMS |  | int |
| 0x1139 | IsActorEvil |  | yes |  |
| 0x113A | IsActorAVictim |  | yes |  |
| 0x113B | GetTotalPersuasionNumber |  |  |  |
| 0x113C | SetScale |  | yes | float |
| 0x113D | ModScale |  | yes | float |
| 0x113E | GetIdleDoneOnce |  |  |  |
| 0x113F | KillAllActors | killall |  | [actor] |
| 0x1140 | GetNoRumors |  | yes |  |
| 0x1141 | SetNoRumors |  | yes | int |
| 0x1142 | Dispel |  | yes | magic item |
| 0x1143 | WhichServiceMenu |  |  |  |
| 0x1144 | TriggerHitShader | ths |  | [float] |
| 0x1145 | RefreshTopicList |  |  |  |
| 0x1146 | Reset3DState |  |  |  |
| 0x1147 | IsRidingHorse |  | yes |  |
| 0x1148 | DispelAllSpells |  | yes |  |
| 0x1149 | IsTurnArrest |  |  |  |
| 0x114A | AddAchievement |  |  | int |
| 0x114B | DuplicateAllItems |  | yes | [container], [int] |
| 0x114C | IsInDangerousWater |  | yes |  |
| 0x114D | EssentialDeathReload |  |  | string |
| 0x114E | SetShowQuestItems |  |  | int |
| 0x114F | DuplicateNPCStats |  | yes | [actor] |
| 0x1150 | ResetHealth |  | yes |  |
| 0x1151 | SetIgnoreFriendlyHits | sifh | yes | int |
| 0x1152 | GetIgnoreFriendlyHits | gifh | yes |  |
| 0x1153 | IsPlayersLastRiddenHorse |  | yes |  |
| 0x1154 | SetActorRefraction | sar | yes | float |
| 0x1155 | SetItemValue |  | yes | int |
| 0x1156 | SetRigidBodyMass |  |  | float |
| 0x1157 | ShowViewerStrings | svs | yes |  |
| 0x1158 | ReleaseWeatherOverride | rwo |  |  |
| 0x1159 | SetAllReachable |  |  | int |
| 0x115A | SetAllVisible |  |  | int |
| 0x115B | SetNoAvoidance |  |  | int |
| 0x115C | SendTrespassAlarm |  | yes | actor |
| 0x115D | SetSceneIsComplex |  |  | int |
| 0x115E | Autosave |  |  |  |
| 0x115F | StartMasterFileSeekData |  |  |  |
| 0x1160 | DumpMasterFileSeekData |  |  |  |
| 0x1161 | IsActor |  | yes |  |
| 0x1162 | IsEssential |  | yes |  |
| 0x1163 | PreloadMagicEffect |  | yes | magic effect |
| 0x1164 | ShowDialogSubtitles |  |  | [int] |
| 0x1165 | ForceCloseOblivionGate |  |  | [int] |
| 0x1166 | IsPlayerMovingIntoNewSpace |  |  |  |
| 0x1167 | CreateFullActorCopy |  | yes |  |
| 0x1168 | DeleteFullActorCopy |  | yes |  |
| 0x1169 | GetTimeDead |  | yes |  |
| 0x116A | GetPlayerHasLastRiddenHorse |  |  |  |
| 0x116B | PurgeCellBuffers | pcb |  |  |
| 0x116C | SetPlayerInSEWorld | SPInSE |  | int |
| 0x116D | GetPlayerInSEWorld | gpInSE |  |  |
| 0x116E | PushActorAway |  | yes | reference, int |
| 0x116F | SetActorsAI |  | yes | int |
| 0x1170 | ClearOwnership |  | yes |  |

## Console commands

| Opcode | Name | Short | Ref | Parameters |
|---|---|---|---|---|
| 0x0100 | Show | TST |  | float |
| 0x0101 | ShowVars | SV | yes |  |
| 0x0102 | ShowQuestVars | SQV |  | quest |
| 0x0103 | ShowQuests | SQ |  |  |
| 0x0104 | ToggleCombatStats | TCS |  |  |
| 0x0105 | ToggleSafeZone | TSZ |  |  |
| 0x0106 | ToggleAI | TAI |  | [actor] |
| 0x0107 | ToggleCollisionGeometry | TCG |  |  |
| 0x0108 | ToggleMaterialGeometry | TMG |  |  |
| 0x0109 | ToggleBorders | TB |  |  |
| 0x010A | ToggleSky | TS |  |  |
| 0x010B | ToggleLeaves | TLV |  |  |
| 0x010C | ToggleWireframe | TWF |  |  |
| 0x010D | ToggleCollision | TCL |  |  |
| 0x010E | ToggleDebugText | TDT |  |  |
| 0x010F | ToggleMenus | TM |  |  |
| 0x0110 | ShowScenegraph | SSG |  |  |
| 0x0111 | ToggleMagicStats | TMS |  |  |
| 0x0112 | ShowAnim | SA | yes |  |
| 0x0113 | Show1stPerson | S1ST |  |  |
| 0x0114 | Help |  |  |  |
| 0x0115 | TogglePathGrid | TPG |  |  |
| 0x0116 | TogglePathLine | TPL |  |  |
| 0x0117 | PickRefByID | PRID |  | reference |
| 0x0118 | SetLightingPasses | SLP |  | string |
| 0x0119 | ToggleLODLand | TLL |  |  |
| 0x011A | ToggleGrassUpdate | TGU |  |  |
| 0x011B | CenterOnCell | COC |  | string |
| 0x011C | SetGameSetting | SetGS |  | string, string |
| 0x011D | SetINISetting | SetINI |  | string, string |
| 0x011E | GetINISetting | GetINI |  | string |
| 0x011F | CenterOnExterior | COE |  | int, int |
| 0x0120 | CalcPathToPoint | P2P | yes |  |
| 0x0121 | CalcLowPathToPoint | LP2P | yes | int, int, int |
| 0x0122 | SetFog |  |  | int, int |
| 0x0123 | SetClipDist |  |  | float |
| 0x0124 | ToggleShadowVolumes | TSV |  |  |
| 0x0125 | SetImageSpaceGlow | SISG |  | int, float, float, float |
| 0x0126 | ToggleDetection | TDETECT |  |  |
| 0x0127 | ToggleHighProcess | THIGHPROCESS |  |  |
| 0x0128 | ToggleLowProcess | TLOWPROCESS |  |  |
| 0x0129 | ToggleMiddleLowProcess | TMLOWPROCESS |  |  |
| 0x012A | ToggleMiddleHighProcess | TMHIGHPROCESS |  |  |
| 0x012B | ToggleAiSchedules | TAIS |  |  |
| 0x012C | SpeakSound |  | yes | string, [int], [int] |
| 0x012D | BetaComment | BC |  | string |
| 0x012E | ToggleCellNode | TCN |  | int |
| 0x012F | ToggleTrees | TT |  |  |
| 0x0130 | SetCameraFOV | FOV |  | int |
| 0x0131 | Verbose | VERBOSE |  |  |
| 0x0132 | ToggleGodMode | TGM |  |  |
| 0x0133 | RefreshShaders | REFRESHSHADERS |  |  |
| 0x0134 | ToggleScripts | TSCR |  |  |
| 0x0135 | SexChange |  |  |  |
| 0x0136 | ToggleConversations | TCONV |  |  |
| 0x0137 | ToggleFullHelp | TFH |  |  |
| 0x0138 | ShowQuestLog | SQL |  | [int] |
| 0x0139 | ShowFullQuestLog | SFQL |  | quest |
| 0x013A | DumpTexturePalette | DTP |  | [string] |
| 0x013B | CenterOnWorld | COW |  | worldspace, int, int |
| 0x013C | ToggleGrass | TG |  |  |
| 0x013D | AddFaceAnimNote | AFAN | yes | string |
| 0x013E | RefreshINI | REFINI |  |  |
| 0x013F | ToggleEmotions | temo |  |  |
| 0x0140 | AddDecal |  |  |  |
| 0x0141 | TestCode |  |  |  |
| 0x0142 | ToggleMapMarkers | tmm |  | int |
| 0x0143 | PlayerSpellBook | psb |  |  |
| 0x0144 | PrintAiList | pai |  |  |
| 0x0145 | ShowQuestTargets | SQT |  |  |
| 0x0146 | HairTint |  |  | int, int, int |
| 0x0147 | SaveGame | save |  | string, [int] |
| 0x0148 | LoadGame | load |  | string, [int] |
| 0x0149 | TestAllCells | TAC |  | int |
| 0x014A | SaveWorld |  |  | string |
| 0x014B | ReloadCurrentClimate | rcc |  |  |
| 0x014C | ReloadCurrentWeather | rcw |  |  |
| 0x014D | TestSeenData | tsd |  |  |
| 0x014E | TestLocalMap | tlm |  | int |
| 0x014F | MoveToQuestTarget | movetoqt |  | [int] |
| 0x0150 | PrintNPCDialog | pdialog |  |  |
| 0x0151 | ShowSubtitle |  |  | [int] |
| 0x0152 | ShowRenderPasses | srp |  |  |
| 0x0153 | FreezeRenderAccumulation | fra |  |  |
| 0x0154 | ToggleOcclusion | tocc |  |  |
| 0x0155 | OutputMemContexts | omc |  | [string] |
| 0x0156 | OutputMemStats | oms |  | [string] |
| 0x0157 | ToggleDetectionStats | tds |  | [actor] |
| 0x0158 | SetTargetRefraction | str |  | float |
| 0x0159 | SetTargetRefractionFire | strf |  | float, int |
| 0x015A | ToggleRefractionDebug | trd |  |  |
| 0x015B | ToggleCharControllerShape | TCCS |  |  |
| 0x015C | ShowHeadTrackTarget | SHeadT | yes |  |
| 0x015D | SetSkyParam | SSP |  | float, float |
| 0x015E | SetHDRParam | SHP |  | float, float, float, float, float, float |
| 0x015F | ShowPivot | SP | yes |  |
| 0x0160 | PrintHDRParam | PHP |  |  |
| 0x0161 | ToggleHDRDebug | THD |  |  |
| 0x0162 | RevertWorld | rw |  |  |
| 0x0163 | OutputArchiveProfile | oap |  | [string] |
| 0x0164 | QuitGame | qqq |  |  |
| 0x0165 | SaveIniFiles | saveini |  |  |
| 0x0166 | SetDebugText | sdt |  | int |
| 0x0167 | ToggleLiteBrite | tlb |  |  |
| 0x0168 | RunMemoryPass | rmp |  | int |
| 0x0169 | ResetMemContexts | rmc |  |  |
| 0x016A | ToggleWaterSystem | TWS |  |  |
| 0x016B | ToggleWaterRadius | TWR |  |  |
| 0x016C | ShowWhoDetectsPlayer | SWDP |  |  |
| 0x016D | ToggleCombatAI | tcai |  |  |
| 0x016E | ToggleFlyCam | tfc |  |  |
| 0x016F | ModWaterShader | mws |  | string, [float] |
| 0x0170 | WaterShallowColor | shallow |  | int, int, int |
| 0x0171 | WaterDeepColor | deep |  | int, int, int |
| 0x0172 | WaterReflectionColor | refl |  | int, int, int |
| 0x0173 | BeginTrace | bt |  |  |
| 0x0174 | RunCellTest | rct |  | [int] |
| 0x0175 | StartAllQuests | saq |  |  |
| 0x0176 | CompleteAllQuestStages | caqs |  |  |
| 0x0177 | FlushNonPersistActors | Flush |  | int |
| 0x0178 | ToggleFogOfWar | TFOW |  |  |
| 0x0179 | OutputLocalMapPictures | OLMP |  |  |
| 0x017A | SetGamma | sg |  | float |
| 0x017B | WasteMemory | wm |  | float |
| 0x017C | ClearAdaptedLight | cal |  |  |
| 0x017D | ToggleCastShadows | tsh |  | int |
| 0x017E | ShowSubSpaces | sss |  |  |
| 0x017F | SetSTBBColorConstants | sscc |  | [float], [float], [float], [float] |
| 0x0180 | CloseFile |  |  | string |
| 0x0181 | SetTreeMipmapBias | stmb |  | [float], [float] |
| 0x0182 | ClearFaceGenModelData | CFG |  |  |