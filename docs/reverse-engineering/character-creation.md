# Character creation

The race, class and birthsign screens are opened by the tutorial's scripts
(`ShowRaceMenu`, `ShowClassMenu`, `ShowBirthSignMenu`; see [new-game.md](new-game.md)).
This note covers the engine logic behind them. The starting attributes and skills that
follow from race and class are in
[combat-and-levelling.md](combat-and-levelling.md#auto-calculated-stats). Confidence tags
as in the README.

## The suggested class

Before the player picks a class, the player has the placeholder class
`CharactergenClass` (`000230E6`, set by `iClassCharactergenClass`). While that is still
the player's class, opening the class menu suggests a class from **how the player has
used their skills so far** (in the tutorial dungeon). **[high]**

```
for each of the 21 skills with skill-use progress p > 0:
    add p to the total of the skill's specialization (combat, magic or stealth)
T = combat + magic + stealth
C = round(7 × combat / T)          # round half up
M = round(7 × magic  / T)
S = round(7 × stealth / T)
```

`p` is the player's accumulated skill-use progress toward the next skill level.
**[medium]** for exactly which counter it is; the arithmetic is **[exact]**.

The class is then picked. Later rules override earlier ones:

```
if C ≥ 7: Warrior
elif M ≥ 7: Mage
elif S ≥ 7: Thief
else:
    by C:  3 → Rogue if M = 2 and S = 2
           4 → Scout if S = 2, else Archer
           5 → Crusader if M = 2, else Knight
           6 → Barbarian
    by M:  3 → Nightblade if C = 2 and S = 2
           4 → Witchhunter if S = 2, else Spellsword
           5 → Healer if S = 2, else Battlemage
           6 → Sorcerer
    by S:  3 → Bard if C < 4 and M < 4
           4 → Monk if C = 2, else Pilgrim
           5 → Assassin if C = 1, else Acrobat
           6 → Agent
    nothing matched: Acrobat
```

**[exact]** for the table. Each class comes from a game setting holding its form ID
(`iClassWarrior` etc.). No vanilla plugin overrides them:

| Class | Form ID | Class | Form ID |
|-------|---------|-------|---------|
| Acrobat | `000237A8` | Monk | `000237A9` |
| Agent | `00023792` | Nightblade | `00023C08` |
| Archer | `0002378B` | Pilgrim | `00000832` |
| Assassin | `00023793` | Rogue | `0002378C` |
| Barbarian | `00023788` | Scout | `0002378A` |
| Bard | `0001C824` | Sorcerer | `000237AC` |
| Battlemage | `00023997` | Spellsword | `00023C07` |
| Crusader | `00023789` | Thief | `0002378F` |
| Healer | `00000910` | Warrior | `00023787` |
| Knight | `00000836` | Witchhunter | `00023998` |
| Mage | `000105D6` | | |

`iClassPriest` exists but is never suggested. The suggestion is also what the
`GetIsClassDefault` condition compares against. **[high]**

`ShowClassMenu` opens the menu with this suggestion when the player's current class
isn't one of the playable ones; otherwise it opens on the current class. **[medium]**
(the "playable" reading of the class flag)

## Custom class

A custom class is created as the form named by `iPlayerCustomClass`. The custom-class
screen has steps for specialization, favoured attributes and major skills (their order is
not pinned yet), then the name, then a confirmation box
"`<prompt> <class name> <suffix>?`" built from game settings. **[medium]**

## Open

1. The race menu: slider ranges, how face morphs are stored, and what changing race
   does to an existing save.
2. The birthsign menu: confirmation flow (`sConfirmBirthsign1`) and when birthsign
   spells are added.
3. The order of the custom-class steps and the custom class's limits.
