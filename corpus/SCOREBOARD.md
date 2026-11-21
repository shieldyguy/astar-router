# Scoreboard: astar on the corpus

Made by `corpus/scoreboard.py` from `corpus/baseline.jsonl`. The binary: `d7d89bc+dirty` (69 boards), `02be9c9` (6 boards). Budget a board: 30 s, 60 s. KiCad: 10.99.0.
The verdict is the house KiCad's: the plan laid on the bare board, zones refilled, DRC; a board passes when every net landed and no DRC error is new against the bare board's own. astar's own count is never the grade.

**55 of 80 boards whole with 0 new DRC errors** (75 routed and judged; 0 gave no scene; 5 not prepared yet).

## By source

| source | what | boards | routed | pass | nets open | DRC | crash, timeout, refused | no scene | not prepared | nets landed |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| bench | router-bench's five | 5 | 5 | 5 | 0 | 0 | 0 | 0 | 0 | 478 of 478 (100%) |
| krt | KRT's kicad_files/ | 20 | 20 | 10 | 10 | 2 | 0 | 0 | 0 | 1326 of 1962 (68%) |
| house | the house's blocks and fixtures | 33 | 33 | 31 | 2 | 0 | 0 | 0 | 0 | 330 of 337 (98%) |
| qa | the KiCad fork's field tests | 3 | 3 | 3 | 0 | 0 | 0 | 0 | 0 | 80 of 80 (100%) |
| demo | the KiCad fork's demos | 14 | 14 | 6 | 5 | 1 | 3 | 0 | 0 | 783 of 1020 (77%) |
| stress | KRT's stress corpus (open boards, fetched) | 5 | 0 | 0 | 0 | 0 | 0 | 0 | 5 | 0 of 0 |
| **all** |  | 80 | 75 | 55 | 17 | 3 | 3 | 0 | 5 | 2997 of 3877 (77%) |

`nets open` and `DRC` each count the boards that have both. `nets landed`: over the boards KiCad judged; where KiCad's list of unconnected items is cut at 499 the count is an upper bound.

## By failure class

| class | boards | what it means | e.g. |
|---|---:|---|---|
| pass | 55 | every net landed, 0 new DRC errors | ccb, demo_One-Air-Max, demo_complex_hierarchy |
| nets open | 14 | KiCad finds nets not joined; no new DRC error | demo_CM5_MINIMA_3, demo_RoyalBlue54L-Feather, demo_StickHub |
| nets open, DRC | 3 | nets not joined, and DRC errors the bare board does not have | krt_kit-dev-coldfire-xilinx_5213, krt_ulx3s, demo_tinytapeout-demo |
| crash | 3 | astar died | demo_jetson-agx-thor-baseboard, demo_multichannel_mixer, demo_vme-wren |
| not prepared | 5 | fetched or listed, not made bare and exported on this machine yet | stress_bitaxe_ultra, stress_hackrf_one, stress_megadesk |

Of the 17 boards with nets open: 5 land 95% or more of their nets, 1 land 80 to 95%, 7 land 50 to 80%, 4 land under half.
Boards with a net astar called joined that KiCad finds open: 0.
New DRC errors, by kind, as the number of boards that have them: starved_thermal 1, solder_mask_bridge 1, clearance 1.
Crashes: panic at src/passes/drops.rs:100 (2); panic at src/route/access.rs:39 (1).

## The other two judges

- **`astar check`** (astar's own exact check, from the scene and the plan alone) ran on 69 boards. It names the same open nets as KiCad on 67. KiCad finds nets open that it calls joined on 2 (krt_rp2350_fpga_eensy_prePlane, krt_watchy); it finds nets open that KiCad does not on 2 (krt_rp2350_fpga_eensy_prePlane, krt_watchy). A net in pieces that the plan itself called joined: 0 boards. The plan's copper breaks the scene's own rules (clearance, holes, edge, keep-outs) on 0 boards.

## Every board

`L` copper layers; `landed` of `nets` by KiCad (`<=`: KiCad's list was cut at 499 items); `DRC` new errors; `s` astar's own seconds.

### bench: router-bench's five (5 of 5 pass)

| board | L | nets | landed | DRC | s | verdict | reason |
|---|---:|---:|---:|---:|---:|---|---|
| ccb | 4 | 47 | 47 | 0 | 0.5 | **pass** |  |
| dock | 4 | 28 | 28 | 0 | 1.3 | **pass** |  |
| g474 | 4 | 42 | 42 | 0 | 0.1 | **pass** |  |
| tb150 | 4 | 118 | 118 | 0 | 1.0 | **pass** |  |
| ub400 | 4 | 243 | 243 | 0 | 6.8 | **pass** |  |

### krt: KRT's kicad_files/ (10 of 20 pass)

| board | L | nets | landed | DRC | s | verdict | reason |
|---|---:|---:|---:|---:|---:|---|---|
| krt_cap_chain | 2 | 4 | 4 | 0 | 0.0 | **pass** |  |
| krt_esp_prog | 2 | 17 | 17 | 0 | 0.3 | **pass** |  |
| krt_flat_hierarchy | 2 | 34 | 34 | 0 | 0.4 | **pass** |  |
| krt_haasoscope_pro_max_test | 10 | 228 | 14 | 0 | 30.2 | nets open | 214 open |
| krt_interf_u_unrouted | 2 | 110 | 110 | 0 | 0.3 | **pass** |  |
| krt_interf_u_unrouted_placed | 2 | 110 | 109 | 0 | 7.6 | nets open | 1 open |
| krt_kit-dev-coldfire-xilinx_5213 | 4 | 209 | 202 | 3 | 30.1 | nets open, DRC | 7 open; 3 starved_thermal |
| krt_lvds_converter_dualclk | 2 | 14 | 14 | 0 | 0.0 | **pass** |  |
| krt_lvds_converter_dualclk_gnd | 2 | 14 | 14 | 0 | 0.0 | **pass** |  |
| krt_orangecrab_ext_pll | 6 | 154 | 35 | 0 | 30.9 | nets open | 119 open |
| krt_qfn_csi_underpad_diff | 4 | 4 | 4 | 0 | 0.0 | **pass** |  |
| krt_qfn_interior_pads | 2 | 2 | 2 | 0 | 0.0 | **pass** |  |
| krt_routed_output | 10 | 228 | 227 | 0 | 30.4 | nets open | 1 open |
| krt_rp2350_fpga_eensy_prePlane | 6 | 74 | 34 | 0 | 30.2 | nets open | 42 open |
| krt_sonde_u | 2 | 26 | 26 | 0 | 0.1 | **pass** |  |
| krt_splitflap_driver | 2 | 83 | 83 | 0 | 0.2 | **pass** |  |
| krt_tigard | 4 | 85 | 84 | 0 | 5.0 | nets open | 1 open |
| krt_ulx3s | 4 | 287 | 132 | 48 | 53.4 | nets open, DRC | 155 open; 48 solder_mask_bridge |
| krt_watchy | 4 | 53 | 27 | 0 | 0.4 | nets open | 29 open; blind to solder_mask_bridge |
| krt_glasgow_revC | 4 | 226 | <=154 | 0 | 0.1 | nets open | 72+ open |

### house: the house's blocks and fixtures (31 of 33 pass)

| board | L | nets | landed | DRC | s | verdict | reason |
|---|---:|---:|---:|---:|---:|---|---|
| fix_lakehouse.placed | 2 | 42 | 42 | 0 | 0.7 | **pass** |  |
| fix_ldo_against | 2 | 3 | 3 | 0 | 0.0 | **pass** |  |
| fix_ldo_drops | 2 | 3 | 3 | 0 | 0.0 | **pass** |  |
| fix_pots.routed | 2 | 15 | 15 | 0 | 0.1 | **pass** |  |
| fix_putlines-host | 2 | 3 | 3 | 0 | 0.0 | **pass** |  |
| fix_putlines-ldo | 2 | 3 | 3 | 0 | 0.0 | **pass** |  |
| fix_ready-pier | 2 | 16 | 16 | 0 | 0.0 | **pass** |  |
| fix_score-ch32-hands | 2 | 16 | 16 | 0 | 0.1 | **pass** |  |
| fix_score-ch32-tuck | 2 | 16 | 16 | 0 | 0.1 | **pass** |  |
| fix_score-ldo-tuck | 2 | 3 | 3 | 0 | 0.0 | **pass** |  |
| fix_tia_placed | 2 | 4 | 4 | 0 | 0.0 | **pass** |  |
| fix_usb_beside | 2 | 7 | 7 | 0 | 0.0 | **pass** |  |
| house_buck-ap63203-3v3 | 2 | 6 | 6 | 0 | 0.0 | **pass** |  |
| house_buck-xl1509-5v | 2 | 3 | 3 | 0 | 0.0 | **pass** |  |
| house_buoy-corner-ch32x035-5v | 2 | 19 | 19 | 0 | 0.1 | **pass** |  |
| house_ch32x035g8u6-core | 2 | 4 | 4 | 0 | 0.0 | **pass** |  |
| house_ch32x035g8u6-jstsh-3v3 | 2 | 23 | 23 | 0 | 0.1 | **pass** |  |
| house_ch32x035g8u6-usb-3v3 | 2 | 16 | 16 | 0 | 0.0 | **pass** |  |
| house_ch32x035g8u6-usb-5v | 2 | 15 | 15 | 0 | 0.0 | **pass** |  |
| house_esp32s3-wroom-core | 2 | 6 | 6 | 0 | 0.0 | **pass** |  |
| house_expo-rail | 2 | 9 | 9 | 0 | 0.0 | **pass** |  |
| house_expo-single-supply | 2 | 9 | 9 | 0 | 0.0 | **pass** |  |
| house_hbridge-drv8870 | 2 | 5 | 5 | 0 | 0.0 | **pass** |  |
| house_hub-ch334p | 2 | 5 | 5 | 0 | 0.0 | **pass** |  |
| house_isense-ina180a1 | 2 | 2 | 2 | 0 | 0.0 | **pass** |  |
| house_keys37-ch32x035 | 2 | 24 | 18 | 0 | 6.4 | nets open | 6 open |
| house_ldo-ams1117-3v3 | 2 | 3 | 2 | 0 | 0.0 | nets open | 1 open |
| house_powerin-pfet-tvs | 2 | 4 | 4 | 0 | 0.0 | **pass** |  |
| house_prog-stdc14 | 2 | 3 | 3 | 0 | 0.0 | **pass** |  |
| house_solenoid-lowside | 2 | 5 | 5 | 0 | 0.0 | **pass** |  |
| house_stepper-driver | 2 | 15 | 15 | 0 | 0.0 | **pass** |  |
| house_svf-bjt-3v3 | 2 | 24 | 24 | 0 | 0.0 | **pass** |  |
| house_usb-c-esd-5v | 2 | 6 | 6 | 0 | 0.0 | **pass** |  |

### qa: the KiCad fork's field tests (3 of 3 pass)

| board | L | nets | landed | DRC | s | verdict | reason |
|---|---:|---:|---:|---:|---:|---|---|
| qa_field_test | 4 | 14 | 14 | 0 | 0.0 | **pass** |  |
| qa_keys-mini | 4 | 33 | 33 | 0 | 0.7 | **pass** |  |
| qa_keys-mini-peel | 4 | 33 | 33 | 0 | 0.9 | **pass** |  |

### demo: the KiCad fork's demos (6 of 14 pass)

| board | L | nets | landed | DRC | s | verdict | reason |
|---|---:|---:|---:|---:|---:|---|---|
| demo_CM5_MINIMA_3 | 6 | 95 | 57 | 0 | 0.4 | nets open | 38 open |
| demo_One-Air-Max | 4 | 120 | 120 | 0 | 1.8 | **pass** |  |
| demo_RoyalBlue54L-Feather | 8 | 69 | 66 | 0 | 5.2 | nets open | 3 open |
| demo_StickHub | 2 | 45 | 40 | 0 | 6.3 | nets open | 5 open |
| demo_complex_hierarchy | 2 | 50 | 50 | 0 | 0.2 | **pass** |  |
| demo_ecc83-pp | 2 | 9 | 9 | 0 | 0.0 | **pass** |  |
| demo_ecc83-pp_v2 | 2 | 9 | 9 | 0 | 0.0 | **pass** |  |
| demo_interf_u | 2 | 110 | 110 | 0 | 1.3 | **pass** |  |
| demo_pic_programmer | 2 | 34 | 34 | 0 | 0.3 | **pass** |  |
| demo_jetson-agx-thor-baseboard | 10 | 883 | - | - | - | crash | panic at src/passes/drops.rs:100 |
| demo_multichannel_mixer | 2 | 79 | - | - | - | crash | panic at src/route/access.rs:39 |
| demo_tinytapeout-demo | 4 | 108 | 69 | 23 | 8.3 | nets open, DRC | 43 open; 23 clearance |
| demo_video | 4 | 371 | 219 | 0 | 70.4 | nets open | 155 open |
| demo_vme-wren | 12 | 1217 | - | - | - | crash | panic at src/passes/drops.rs:100 |

### stress: KRT's stress corpus (open boards, fetched) (0 of 5 pass)

| board | L | nets | landed | DRC | s | verdict | reason |
|---|---:|---:|---:|---:|---:|---|---|
| bitaxe_ultra | 4 | 61 | - | - | - | not prepared |  |
| hackrf_one | 4 | 319 | - | - | - | not prepared |  |
| megadesk | 2 | 24 | - | - | - | not prepared |  |
| piantor | 2 | 27 | - | - | - | not prepared |  |
| urchin | 2 | 68 | - | - | - | not prepared |  |

The stress boards are third-party and not in this repository: `corpus/manifest.json` names each one's repository, commit and path, `corpus/fetch_stress.py` fetches them, `corpus/prepare.py --source stress` makes them bare and exports their scenes.
