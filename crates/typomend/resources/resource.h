// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// Resource identifiers shared by typomend.rc and src/dialog/ids.rs.
// Keep both files in sync when adding or renumbering an id.
//
// IDD_* name the dialog templates, 1xx. IDC_* name the controls, numbered in
// blocks of one hundred per page, starting at 1001.

// Icons, from the .ico files in this directory. IDI_APP has the lowest id
// because Explorer shows the first icon in the executable.
#define IDI_APP                 1
#define IDI_TRAY_INK            2
#define IDI_TRAY_INK_PAUSED     3
#define IDI_TRAY_WHITE          4
#define IDI_TRAY_WHITE_PAUSED   5

#define IDD_OVERVIEW            101
#define IDD_RULES               102
#define IDD_APPS                103
#define IDD_HISTORY             104
#define IDD_SETTINGS            105
#define IDD_ABOUT               106
#define IDD_MODEL               107
#define IDD_USAGE                108
#define IDD_TRY                 109
#define IDD_BENCH               110

#define IDC_ENABLED             1001
#define IDC_STATUS              1002
#define IDC_FOCUS_LABEL         1003
#define IDC_FOCUS               1004
#define IDC_COUNT               1005

#define IDC_ENGINE_LABEL        1101
#define IDC_ENGINE_RULES        1102
#define IDC_ENGINE_MLM          1103
#define IDC_RULES_FILE          1104
#define IDC_RULES_STATUS        1105
#define IDC_RULES               1106
#define IDC_EDIT_RULES          1107
#define IDC_OPEN_RULES_FOLDER   1108

#define IDC_APPS_INTRO          1201
#define IDC_APPS                1202
#define IDC_APP_MODE_HINT       1203
#define IDC_APP_MODE_LABEL      1204
#define IDC_APP_MODE            1205

#define IDC_HISTORY_INTRO       1301
#define IDC_HISTORY             1302
#define IDC_HISTORY_EMPTY       1303
#define IDC_CLEAR               1304

#define IDC_LANGUAGE_LABEL      1401
#define IDC_LANGUAGE            1402
#define IDC_AUTOSTART           1403
#define IDC_PRIVATE             1404
#define IDC_IDLE_LABEL          1405
#define IDC_IDLE                1406
#define IDC_IDLE_SPIN           1407
#define IDC_IDLE_HINT           1408
#define IDC_CARET_LABEL         1409
#define IDC_CARET               1410
#define IDC_CARET_SPIN          1411
#define IDC_BLIND               1412
#define IDC_BLIND_HINT          1413
#define IDC_OPEN_LOGS           1414
#define IDC_PRIVATE_HINT        1415
#define IDC_DELETE_LOGS         1416

#define IDC_ABOUT_NAME          1501
#define IDC_ABOUT_VERSION       1502
#define IDC_ABOUT_DESCRIPTION   1503
#define IDC_ABOUT_LICENSE       1504
#define IDC_ABOUT_LINK          1505
#define IDC_ABOUT_LOGO          1506
#define IDC_ABOUT_TAGLINE       1507

#define IDC_LM_INTRO            1601
#define IDC_LM_MODEL_LABEL      1602
#define IDC_LM_MODEL            1603
#define IDC_LM_MODE_LABEL       1604
#define IDC_LM_MODE             1605
#define IDC_LM_STATUS           1606
#define IDC_LM_DOWNLOAD         1607
#define IDC_LM_DELETE           1608
#define IDC_LM_DIR              1609
#define IDC_LM_CHANGE_DIR       1610
#define IDC_OPEN_MODELS_FOLDER  1611
#define IDC_LM_NOTE             1612
#define IDC_LM_BENCH            1613
#define IDC_LM_EXPECTED         1614
#define IDC_LM_TRY              1615

#define IDC_USAGE_ENABLED        1701
#define IDC_USAGE_HINT           1702
#define IDC_USAGE_MODEL          1703
#define IDC_USAGE_CPU            1704
#define IDC_USAGE_MEMORY         1705
#define IDC_USAGE_GPU            1706

#define IDC_TRY_INTRO           1801
#define IDC_TRY_TEXT            1802
#define IDC_TRY_RUN             1803
#define IDC_TRY_RESULT          1804
#define IDC_TRY_LIST            1805

#define IDC_BENCH_INTRO         1901
#define IDC_BENCH_MODEL         1902
#define IDC_BENCH_RUN           1903
#define IDC_BENCH_PROGRESS      1904
#define IDC_BENCH_STATUS        1905
#define IDC_BENCH_LIST          1906
