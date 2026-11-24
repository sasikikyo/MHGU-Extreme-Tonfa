# MHGU Extreme Tonfa

A work-in-progress port of Monster Hunter Frontier's Extreme Style Tonfa (穿龙棍) for MHGU Recomp. This build targets Recomp 0.18.0 with the tested Definitive Edition executable layout and a Tonfa-compatible Style Registry.

## Play

1. Download the [Tonfa mod ZIP](downloads/Tonfa-Extreme-standalone-Definitive-Edition.zip).
2. Extract the `Tonfa-Extreme` directory into the game's `mods` directory. Keep the matching executable patches together. Use the compatible `0-style-registry` from your setup. Magnet Spike is optional as a development reference and is unnecessary for this build.
3. Restart Recomp, equip a Tonfa and select Extreme Style. The [Xbox and keyboard move chart](Extreme-Tonfa-Xbox-Keyboard.png) lists the inputs.

The discharge sound is still being matched to Frontier. Full combat parity and multiplayer behavior remain unverified.

## Source

The `src` directory contains the Rust combat controller, equipment integration, Style Registry patch and conversion scripts. The `data` directory contains weapon definitions and converted model data. The SDK sources in `vendor` come from the development checkout of [mhgu-mods](https://github.com/super-wzk/mhgu-mods). A full regeneration also requires the external conversion tools, game data and target mod layout used by the build scripts.

The equipment integration draws on [MHGURecomp Magnet Spike](https://github.com/403JT/MHGURecomp-Magnet-Spike). Its upstream notices are in `notices`. Monster Hunter Generations Ultimate and Monster Hunter Frontier are Capcom titles.
