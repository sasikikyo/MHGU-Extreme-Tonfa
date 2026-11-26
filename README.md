# MHGU Extreme Tonfa

A work-in-progress port of Monster Hunter Frontier's Extreme Style Tonfa (穿龙棍) for MHGU Recomp. Tested in offline hunts on Windows MHGU Recomp 0.18.0 and 0.20.0.

## Play

[Download the latest English base-game release](https://github.com/sasikikyo/MHGU-Extreme-Tonfa/releases/latest). The ZIP includes Extreme Style Tonfa, the matching Style Registry and English installation instructions for MHGU 1.4.0.

| Platform | Installation |
| --- | --- |
| Windows Recomp | Open the ZIP's `Recomp` folder. Copy `mods` next to `mhgurecomp.exe`. |
| Physical Switch | With MHGU Forge installed, open the ZIP's `Switch` folder. Merge `atmosphere` into the SD card root. |

When upgrading, back up older copies of Tonfa and Style Registry outside the active mod folders. The included `INSTALL.txt` covers each platform. For the move list, open Hunter's Notes → Weapon Controls → Tonfa, or see the [Xbox and keyboard chart](Extreme-Tonfa-Xbox-Keyboard.png).

The base build has been tested in offline hunts on Windows Recomp 0.20.0. The physical Switch package layout has been verified. Gameplay on physical Switch and Android Relink, full Frontier combat parity and multiplayer remain unverified.

## Source

The `src` directory contains the Rust combat controller, equipment integration, Style Registry patch and conversion scripts. The `data` directory contains weapon definitions and converted model data. The SDK sources in `vendor` come from the development checkout of [mhgu-mods](https://github.com/super-wzk/mhgu-mods). A full regeneration also requires the external conversion tools, game data and target mod layout used by the build scripts.

The equipment integration draws on [MHGURecomp Magnet Spike](https://github.com/403JT/MHGURecomp-Magnet-Spike). Its upstream notices are in `notices`. Monster Hunter Generations Ultimate and Monster Hunter Frontier are Capcom titles.
