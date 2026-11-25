# MHGU Extreme Tonfa

A work-in-progress port of Monster Hunter Frontier's Extreme Style Tonfa (穿龙棍) for MHGU Recomp. Tested in offline hunts on Windows MHGU Recomp 0.18.0 and 0.20.0.

## Play

1. Choose the Tonfa ZIP for your active mod setup.

   | Your setup | Download |
   | --- | --- |
   | Base Recomp, including setups with the Extreme Style mods | [Tonfa for base Recomp](downloads/Tonfa-Extreme-Base-Recomp.zip?raw=true) |
   | MHGU Definitive Edition enabled | [Tonfa for Definitive Edition](downloads/Tonfa-Extreme-standalone-Definitive-Edition.zip?raw=true) |

   Install one Tonfa ZIP. Each includes a matching pair of executable patches. Mixing the files or choosing the wrong layout can prevent the game from launching.
2. Download the [Tonfa-compatible Style Registry](downloads/0-style-registry-Tonfa-compatible.zip?raw=true). With Recomp closed, extract both ZIPs into the game's `mods` directory. If you already have a registry, back up its plugin outside the active mods folder before replacing it. Keep a single active `0-style-registry` alongside `Tonfa-Extreme`.

3. Restart Recomp, equip a Tonfa and select Extreme Style. The [Xbox and keyboard move chart](Extreme-Tonfa-Xbox-Keyboard.png) lists the inputs.

On 0.20.0, Tonfa registered on weapon key 16 and landed hits with all six Extreme Style plugins loaded in both setups. Extreme Dual Blades retains its own style. The upstream registry supplied for this test rejects key 16 and logs `Extreme Tonfa registration failed: Invalid`. If the game launches but the Tonfa moveset is missing, check which registry is active. Emulator compatibility is untested.

The discharge sound is still being matched to Frontier. Full combat parity and multiplayer behavior remain unverified.

## Source

The `src` directory contains the Rust combat controller, equipment integration, Style Registry patch and conversion scripts. The `data` directory contains weapon definitions and converted model data. The SDK sources in `vendor` come from the development checkout of [mhgu-mods](https://github.com/super-wzk/mhgu-mods). A full regeneration also requires the external conversion tools, game data and target mod layout used by the build scripts.

The equipment integration draws on [MHGURecomp Magnet Spike](https://github.com/403JT/MHGURecomp-Magnet-Spike). Its upstream notices are in `notices`. Monster Hunter Generations Ultimate and Monster Hunter Frontier are Capcom titles.
