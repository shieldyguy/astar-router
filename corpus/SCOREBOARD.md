# Scoreboard: astar on the corpus

Made by `corpus/scoreboard.py` from `corpus/baseline.jsonl`. The binary: `f5eb702` (74 boards), `02be9c9` (6 boards). Budget a board: 30 s, 60 s, 30 s (stress). KiCad: 10.99.0.
The verdict is the house KiCad's: the plan laid on the bare board, zones refilled, DRC; a board passes when every net landed and no DRC error is new against the bare board's own. astar's own count is never the grade.

**58 of 80 boards whole with 0 new DRC errors** (80 routed and judged; 0 gave no scene; 0 not prepared yet).

## By source

| source | what | boards | routed | pass | nets open | DRC | crash, timeout, refused | no scene | not prepared | nets landed |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| bench | router-bench's five | 5 | 5 | 5 | 0 | 0 | 0 | 0 | 0 | 478 of 478 (100%) |
| krt | KRT's kicad_files/ | 20 | 20 | 11 | 9 | 2 | 0 | 0 | 0 | 1310 of 1962 (67%) |
| house | the house's blocks and fixtures | 33 | 33 | 30 | 2 | 1 | 0 | 0 | 0 | 335 of 337 (99%) |
| qa | the KiCad fork's field tests | 3 | 3 | 3 | 0 | 0 | 0 | 0 | 0 | 80 of 80 (100%) |
| demo | the KiCad fork's demos | 14 | 14 | 6 | 5 | 1 | 3 | 0 | 0 | 784 of 1020 (77%) |
| stress | KRT's stress corpus (open boards, fetched) | 5 | 5 | 3 | 2 | 1 | 0 | 0 | 0 | 501 of 505 (99%) |
| **all** |  | 80 | 80 | 58 | 18 | 5 | 3 | 0 | 0 | 3488 of 4382 (80%) |

`nets open` and `DRC` each count the boards that have both. `nets landed`: over the boards KiCad judged; where KiCad's list of unconnected items is cut at 499 the count is an upper bound.

## By failure class

| class | boards | what it means | e.g. |
|---|---:|---|---|
| pass | 58 | every net landed, 0 new DRC errors | dock, g474, ccb |
| nets open | 14 | KiCad finds nets not joined; no new DRC error | krt_glasgow_revC, krt_haasoscope_pro_max_test, krt_orangecrab_ext_pll |
| nets open, DRC | 4 | nets not joined, and DRC errors the bare board does not have | krt_kit-dev-coldfire-xilinx_5213, krt_ulx3s, demo_tinytapeout-demo |
| DRC | 1 | every net landed, with DRC errors the bare board does not have | fix_pots.routed |
| crash | 3 | astar died | demo_jetson-agx-thor-baseboard, demo_multichannel_mixer, demo_vme-wren |

Of the 18 boards with nets open: 5 land 95% or more of their nets, 3 land 80 to 95%, 6 land 50 to 80%, 4 land under half.
Boards with a net astar called joined that KiCad finds open: 1 (stress_hackrf_one).
New DRC errors, by kind, as the number of boards that have them: starved_thermal 3, solder_mask_bridge 2, clearance 1.
Crashes: panic at src/passes/drops.rs:100 (2); panic at src/route/access.rs:39 (1).

## The other two judges

- **`astar check`** (astar's own exact check, from the scene and the plan alone) ran on 74 boards. It names the same open nets as KiCad on 70. KiCad finds nets open that it calls joined on 4 (krt_kit-dev-coldfire-xilinx_5213, krt_rp2350_fpga_eensy_prePlane, krt_watchy, stress_hackrf_one); it finds nets open that KiCad does not on 3 (krt_kit-dev-coldfire-xilinx_5213, krt_rp2350_fpga_eensy_prePlane, krt_watchy). A net in pieces that the plan itself called joined: 0 boards. The plan's copper breaks the scene's own rules (clearance, holes, edge, keep-outs) on 0 boards.

## Every board

`L` copper layers; `landed` of `nets` by KiCad (`<=`: KiCad's list was cut at 499 items); `DRC` new errors; `s` astar's own seconds.

### bench: router-bench's five (5 of 5 pass)

| board | L | nets | landed | DRC | s | verdict | reason |
|---|---:|---:|---:|---:|---:|---|---|
| dock | 4 | 28 | 28 | 0 | 6.0 | **pass** |  |
| g474 | 4 | 42 | 42 | 0 | 0.2 | **pass** |  |
| ccb | 4 | 47 | 47 | 0 | 0.8 | **pass** |  |
| tb150 | 4 | 118 | 118 | 0 | 6.7 | **pass** |  |
| ub400 | 4 | 243 | 243 | 0 | 14.8 | **pass** |  |

### krt: KRT's kicad_files/ (11 of 20 pass)

| board | L | nets | landed | DRC | s | verdict | reason |
|---|---:|---:|---:|---:|---:|---|---|
| krt_cap_chain | 2 | 4 | 4 | 0 | 0.0 | **pass** |  |
| krt_esp_prog | 2 | 17 | 17 | 0 | 1.5 | **pass** |  |
| krt_flat_hierarchy | 2 | 34 | 34 | 0 | 0.5 | **pass** |  |
| krt_glasgow_revC | 4 | 226 | <=154 | 0 | 0.1 | nets open | 72+ open |
| krt_haasoscope_pro_max_test | 10 | 228 | 13 | 0 | 32.2 | nets open | 215 open |
| krt_interf_u_unrouted | 2 | 110 | 110 | 0 | 1.1 | **pass** |  |
| krt_interf_u_unrouted_placed | 2 | 110 | 110 | 0 | 3.6 | **pass** |  |
| krt_kit-dev-coldfire-xilinx_5213 | 4 | 209 | 186 | 1 | 33.9 | nets open, DRC | 24 open; 1 starved_thermal |
| krt_lvds_converter_dualclk | 2 | 14 | 14 | 0 | 0.1 | **pass** |  |
| krt_lvds_converter_dualclk_gnd | 2 | 14 | 14 | 0 | 0.1 | **pass** |  |
| krt_orangecrab_ext_pll | 6 | 154 | 33 | 0 | 33.4 | nets open | 121 open |
| krt_qfn_csi_underpad_diff | 4 | 4 | 4 | 0 | 0.0 | **pass** |  |
| krt_qfn_interior_pads | 2 | 2 | 2 | 0 | 0.0 | **pass** |  |
| krt_routed_output | 10 | 228 | 226 | 0 | 37.4 | nets open | 2 open |
| krt_rp2350_fpga_eensy_prePlane | 6 | 74 | 35 | 0 | 30.2 | nets open | 40 open |
| krt_sonde_u | 2 | 26 | 26 | 0 | 0.2 | **pass** |  |
| krt_splitflap_driver | 2 | 83 | 83 | 0 | 1.1 | **pass** |  |
| krt_tigard | 4 | 85 | 82 | 0 | 21.1 | nets open | 3 open |
| krt_ulx3s | 4 | 287 | 136 | 47 | 38.4 | nets open, DRC | 151 open; 47 solder_mask_bridge |
| krt_watchy | 4 | 53 | 27 | 0 | 0.3 | nets open | 29 open; blind to solder_mask_bridge |

### house: the house's blocks and fixtures (30 of 33 pass)

| board | L | nets | landed | DRC | s | verdict | reason |
|---|---:|---:|---:|---:|---:|---|---|
| house_buck-ap63203-3v3 | 2 | 6 | 6 | 0 | 0.0 | **pass** |  |
| house_buck-xl1509-5v | 2 | 3 | 3 | 0 | 0.0 | **pass** |  |
| house_buoy-corner-ch32x035-5v | 2 | 19 | 19 | 0 | 0.2 | **pass** |  |
| house_ch32x035g8u6-core | 2 | 4 | 4 | 0 | 0.0 | **pass** |  |
| house_ch32x035g8u6-jstsh-3v3 | 2 | 23 | 23 | 0 | 0.3 | **pass** |  |
| house_ch32x035g8u6-usb-3v3 | 2 | 16 | 16 | 0 | 0.0 | **pass** |  |
| house_ch32x035g8u6-usb-5v | 2 | 15 | 15 | 0 | 0.0 | **pass** |  |
| house_esp32s3-wroom-core | 2 | 6 | 6 | 0 | 0.0 | **pass** |  |
| house_expo-rail | 2 | 9 | 9 | 0 | 0.0 | **pass** |  |
| house_expo-single-supply | 2 | 9 | 9 | 0 | 0.0 | **pass** |  |
| house_hbridge-drv8870 | 2 | 5 | 5 | 0 | 0.0 | **pass** |  |
| house_hub-ch334p | 2 | 5 | 5 | 0 | 0.0 | **pass** |  |
| house_isense-ina180a1 | 2 | 2 | 2 | 0 | 0.0 | **pass** |  |
| house_keys37-ch32x035 | 2 | 24 | 23 | 0 | 0.1 | nets open | 1 open |
| house_ldo-ams1117-3v3 | 2 | 3 | 2 | 0 | 0.0 | nets open | 1 open |
| house_powerin-pfet-tvs | 2 | 4 | 4 | 0 | 0.0 | **pass** |  |
| house_prog-stdc14 | 2 | 3 | 3 | 0 | 0.0 | **pass** |  |
| house_solenoid-lowside | 2 | 5 | 5 | 0 | 0.0 | **pass** |  |
| house_stepper-driver | 2 | 15 | 15 | 0 | 0.0 | **pass** |  |
| house_svf-bjt-3v3 | 2 | 24 | 24 | 0 | 0.2 | **pass** |  |
| house_usb-c-esd-5v | 2 | 6 | 6 | 0 | 0.0 | **pass** |  |
| fix_lakehouse.placed | 2 | 42 | 42 | 0 | 0.4 | **pass** |  |
| fix_ldo_against | 2 | 3 | 3 | 0 | 0.0 | **pass** |  |
| fix_ldo_drops | 2 | 3 | 3 | 0 | 0.0 | **pass** |  |
| fix_pots.routed | 2 | 15 | 15 | 1 | 0.2 | DRC | 1 starved_thermal |
| fix_tia_placed | 2 | 4 | 4 | 0 | 0.0 | **pass** |  |
| fix_usb_beside | 2 | 7 | 7 | 0 | 0.1 | **pass** |  |
| fix_putlines-host | 2 | 3 | 3 | 0 | 0.0 | **pass** |  |
| fix_putlines-ldo | 2 | 3 | 3 | 0 | 0.0 | **pass** |  |
| fix_ready-pier | 2 | 16 | 16 | 0 | 0.0 | **pass** |  |
| fix_score-ch32-hands | 2 | 16 | 16 | 0 | 0.2 | **pass** |  |
| fix_score-ch32-tuck | 2 | 16 | 16 | 0 | 0.2 | **pass** |  |
| fix_score-ldo-tuck | 2 | 3 | 3 | 0 | 0.0 | **pass** |  |

### qa: the KiCad fork's field tests (3 of 3 pass)

| board | L | nets | landed | DRC | s | verdict | reason |
|---|---:|---:|---:|---:|---:|---|---|
| qa_field_test | 4 | 14 | 14 | 0 | 0.0 | **pass** |  |
| qa_keys-mini-peel | 4 | 33 | 33 | 0 | 2.7 | **pass** |  |
| qa_keys-mini | 4 | 33 | 33 | 0 | 1.5 | **pass** |  |

### demo: the KiCad fork's demos (6 of 14 pass)

| board | L | nets | landed | DRC | s | verdict | reason |
|---|---:|---:|---:|---:|---:|---|---|
| demo_CM5_MINIMA_3 | 6 | 95 | 57 | 0 | 0.4 | nets open | 38 open |
| demo_complex_hierarchy | 2 | 50 | 50 | 0 | 0.2 | **pass** |  |
| demo_ecc83-pp | 2 | 9 | 9 | 0 | 0.0 | **pass** |  |
| demo_ecc83-pp_v2 | 2 | 9 | 9 | 0 | 0.1 | **pass** |  |
| demo_interf_u | 2 | 110 | 110 | 0 | 1.8 | **pass** |  |
| demo_jetson-agx-thor-baseboard | 10 | 883 | - | - | - | crash | panic at src/passes/drops.rs:100 |
| demo_multichannel_mixer | 2 | 79 | - | - | - | crash | panic at src/route/access.rs:39 |
| demo_One-Air-Max | 4 | 120 | 120 | 0 | 2.7 | **pass** |  |
| demo_pic_programmer | 2 | 34 | 34 | 0 | 0.4 | **pass** |  |
| demo_RoyalBlue54L-Feather | 8 | 69 | 66 | 0 | 3.7 | nets open | 3 open |
| demo_StickHub | 2 | 45 | 41 | 0 | 6.5 | nets open | 4 open |
| demo_tinytapeout-demo | 4 | 108 | 69 | 23 | 8.3 | nets open, DRC | 43 open; 23 clearance |
| demo_video | 4 | 371 | 219 | 0 | 70.4 | nets open | 155 open |
| demo_vme-wren | 12 | 1217 | - | - | - | crash | panic at src/passes/drops.rs:100 |

### stress: KRT's stress corpus (open boards, fetched) (3 of 5 pass)

| board | L | nets | landed | DRC | s | verdict | reason |
|---|---:|---:|---:|---:|---:|---|---|
| bitaxe_ultra | 4 | 61 | 61 | 0 | 3.9 | **pass** |  |
| hackrf_one | 4 | 319 | 317 | 5 | 31.8 | nets open, DRC | 2 open; 1 called joined; 3 solder_mask_bridge, 2 starved_thermal |
| megadesk | 2 | 24 | 24 | 0 | 0.1 | **pass** |  |
| piantor | 2 | 33 | 31 | 0 | 10.7 | nets open | 2 open |
| urchin | 2 | 68 | 68 | 0 | 1.5 | **pass** |  |

The stress boards are third-party and not in this repository: `corpus/manifest.json` names each one's repository, commit and path, `corpus/fetch_stress.py` fetches them, `corpus/prepare.py --source stress` makes them bare and exports their scenes.
