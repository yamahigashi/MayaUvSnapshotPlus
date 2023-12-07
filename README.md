# Maya UV Snapshot Plus

## Overview

Maya UV Snapshot Plus is an enhanced UV snapshot tool for Autodesk Maya.
It highlights edges by their type to help you create UV maps that are easier to visualize and understand.

<img src="https://raw.githubusercontent.com/yamahigashi/MayaUvSnapshotPlus/doc/doc/Screenshot_63.png" width="540">


## Key Features

- Hard Edge Display: Emphasizes critical edges of the model.
- UV Seam Detection: Clearly indicates the seams in UV maps. (maya 2023 or later)
- Edge Crease Highlighting: Visually identifies folding or curved edges.
- Display of edges any angle: Edges with two or more sides at a specified angle are displayed.
- Customizable Colors and Widths: Adjust the edge colors and widths according to user preference or specific needs.


## Supported Environments
- **Operating System**: Windows
- **Maya Version**: Autodesk Maya 2020 and later verisons.


## How to Use

1. Open Autodesk Maya.
2. Launch the Maya UV Snapshot Plus tool.
3. Main Menu > Window > `UV Snapshot Plus`
4. Adjust the settings for edge color and width as needed.
5. Click the 'Take Snapshot!' button to generate the UV snapshot.

## Installation
- Download the [zip](https://github.com/yamahigashi/MayaUvSnapshotPlus/releases/download/0.1.1/mayauvsnapshotplus_v0.1.1.zip) file from the [Releases page](https://github.com/yamahigashi/MayaUvSnapshotPlus/releases).
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


## License

This project is published under the [MIT License]().
Support and Contact


----

## 概要

Maya UV Snapshot Plusは、Autodesk Maya用のUVスナップショットツールです。
エッジをその種類ごとに強調表示し、より視覚的に理解しやすいUVマップの作成を支援します。


## 主な特徴

- ハードエッジ表示: モデルの重要なエッジを強調表示。
- UVシーム検出: UVマップの切断線を明確に表示。 (maya2023以降）
- エッジクリース強調: 折りたたみや曲線部分のエッジを視覚的に識別。
- 任意角度のエッジの表示：指定角度以上の2面があるエッジを表示。
- カスタマイズ可能な色と幅: ユーザーの好みや特定のニーズに合わせて、エッジの色と幅を調整可能。


## 使用方法

1. Autodesk Mayaを開きます。
2. Maya UV Snapshot Plusツールを起動します。
3. メインメニュー > ウィンドウ > `UV Snapshot Edge drawer` より起動
4. 必要に応じて、エッジの色や幅の設定を調整します。
5. 「Take Snapshot!」ボタンをクリックして、UVスナップショットを生成します。


## インストール

1. [リリースページ](https://github.com/yamahigashi/MayaUvSnapshotPlus/releases) より [zipファイル](https://github.com/yamahigashi/MayaUvSnapshotPlus/releases/download/0.1.1/mayauvsnapshotplus_v0.1.0.zip) を取得します 
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
3. 作成したフォルダ内に配置します

<img src="https://raw.githubusercontent.com/yamahigashi/MayaUvSnapshotPlus/doc/doc/Screenshot_612.png" width="660">
