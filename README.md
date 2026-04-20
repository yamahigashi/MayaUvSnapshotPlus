# Maya UV Snapshot Plus

## Overview

Maya UV Snapshot Plus is an enhanced UV snapshot tool for Autodesk Maya.
It highlights edges by their type to help you create UV maps that are easier to visualize and understand.

<img src="https://raw.githubusercontent.com/yamahigashi/MayaUvSnapshotPlus/doc/doc/Screenshot_63.png" width="540">


## Key Features

- Padding Overlap Warning: Detects areas that come closer than a specified pixel threshold and highlights them as warnings.
- Direct Clipboard Copy: Sends the snapshot straight to the clipboard for quick use in external paint tools.
- Separate Outer and Inner Line Control: Lets you control UV shell outlines and internal lines independently.
- Hard Edge Display: Emphasizes important model edges for clearer readability.
- UV Seam Detection: Clearly shows UV seams in the snapshot. Available in `Maya 2023` and later.
- Fold Edge Highlighting: Makes edges around folds and stronger surface changes easier to identify.
- Custom Angle Edge Display: Shows edges whose face angle exceeds a user-defined threshold.
- Customizable Colors and Widths: Fine-tune line colors and widths to match your workflow and visibility needs.


## Supported Environments
- **Operating System**: Windows
- **Maya Version**: Autodesk Maya 2022 and later verisons.


## Installation
- Download the [zip](https://github.com/yamahigashi/MayaUvSnapshotPlus/releases/download/v2.0.0/mayauvsnapshotplus_v2.0.0.zip) file from the [Releases page](https://github.com/yamahigashi/MayaUvSnapshotPlus/releases).
- Unzip the downloaded file.
- Place the unzipped files in a folder that is recognized by the `MAYA_MODULE_PATH`, using one of the following methods:

```
a. Place it in the `MyDocuments\maya\modules` folder within your Documents.
b. Place it in any location and register that location in the system's environment variables.
```

If you are not familiar with handling environment variables, method a. is recommended. Here's a detailed explanation for method a.:

- Open the My Documents folder.
- If there is no `modules` folder inside the maya folder, create one.
- Place the unzipped files in this newly created folder.

<img src="https://raw.githubusercontent.com/yamahigashi/MayaUvSnapshotPlus/doc/doc/Screenshot_612.png" width="660">

## Uninstall

1. Exit Maya.
2. Delete the files and folders you placed in the module folder when installing Maya UV Snapshot Plus.




## How to Use

1. Open Autodesk Maya.
2. Launch Maya UV Snapshot Plus from Main Menu > Window > `UV Snapshot Plus`.
3. Adjust the settings for edge color and width as needed.
4. Select a mesh.
5. Click the 'Take Snapshot!' button to generate the UV snapshot.

## Notes

- The UV Snapshot Plus window does not automatically follow selection or mode changes after it has been opened. Reopen the window when needed.
- Performance may degrade significantly on meshes with more than one million polygons.


## License

This project is published under the [MIT License](LICENSE).


----

## 概要

Maya UV Snapshot Plusは、Autodesk Maya用のUVスナップショットツールです。
エッジをその種類ごとに強調表示し、より視覚的に理解しやすいUVマップの作成を支援します。


## 主な特徴

- Padding侵食警告表示: 指定ピクセル以下まで接近した箇所を検出し、警告として表示できます。
- クリップボードへの直接コピー: 一時保存の手間なく、外部ペイントソフトへそのまま貼り付けられます。
- 外部線・内部線の分離: UVシェルの外周と内部の線を個別に制御できます。
- ハードエッジ表示: モデル上で重要なエッジを強調表示できます。
- UVボーダー検出: UVマップの切れ目を明確に表示します。`Maya 2023` 以降で利用できます。
- 折りたたみエッジ強調: 折れや曲面の変化が大きいエッジを視覚的に把握できます。
- 任意角度エッジの表示: 指定した角度以上の面角を持つエッジを抽出して表示できます。
- 色と線幅のカスタマイズ: 用途や見やすさに応じて、表示色と線幅を細かく調整できます。

## インストール

1. [リリースページ](https://github.com/yamahigashi/MayaUvSnapshotPlus/releases) より [zipファイル](https://github.com/yamahigashi/MayaUvSnapshotPlus/releases/download/v2.0.0/mayauvsnapshotplus_v2.0.0.zip) を取得します 
2. ダウンロードしたファイルを解凍します
3. 以下いずれかの方法で `MAYA_MODULE_PATH` の通ったフォルダへ配置します

```
A. `マイドキュメント内 maya\modules` フォルダへ配置する
B. 適当な場所に配置しシステムの環境変数へ登録する
```
    
環境変数の扱いに不慣れであれば a. をおすすめします。
a. について詳解します
1. マイドキュメントフォルダを開く
2. maya内に `modules` フォルダがない場合作成します
3. 作成したフォルダ `modules` 内に解凍したファイル MayaUvSnapshotPlusフォルダおよび、MayaUvSnapshotPlus.mod ファイルを配置します

<img src="https://raw.githubusercontent.com/yamahigashi/MayaUvSnapshotPlus/doc/doc/Screenshot_612.png" width="660">

## アンインストール

1. Mayaを終了します。
2. インストール時に module フォルダへ配置したファイルとフォルダを削除します。


## 使用方法

1. Autodesk Mayaを開きます。
2. Maya UV Snapshot Plusツールを起動します。メインメニュー > ウィンドウ > `UV Snapshot Plus` を実行します
3. ポップしたウィンドにて必要に応じて、エッジの色や幅の設定を調整します。
4. メッシュを選択します。
5. 「Take Snapshot!」ボタンをクリックして、UVスナップショットを生成します。

## 注意事項

- UV Snapshot Plusウィンドウは、表示後に選択状態やモード変更へ自動追従しません。必要に応じてウィンドウを開き直してください。
- 100万ポリゴンを超えるメッシュでは、処理速度が大きく低下する場合があります。
