# reorder-vfat

WindowsでVFAT/exFATディレクトリの順序を並べ替えるツールです。
- USBメモリのFATに登録されている順に再生するMP3プレーヤーで、再生順を変えたいときに
- 低レイヤーでFATを直接書き換えてはいません


## 使い方

1. `reorder-vfat.exe` を起動します。
2. USBメモリのドライブを選択します。
3. 矢印キーで項目を選択します。
4. `INS` / `DEL` でファイルの順序を移動します。
5. `W` でVFAT順序の書き込みを実行します（確認あり）。

主要キー操作は画面右側のヘルプを参照してください。


## 注意

- 失敗することもあり得ます。バックアップは必ず取ってください。
- 取り外し可能ドライブのみ対象です。
- FAT/exFAT以外のドライブには書き込みできません。
- 安全対策は入れていますが、もしできそうに見えてもUSBメモリ以外で実行しないでください。


## ダウンロード

- [Download Latest Version](https://github.com/shibucha256/reorder-vfat/releases/tag/v0.1.0)


## ビルド

```
cargo build --release
```

- zip作成

```
.\scripts\package.ps1
```

## バージョン

- ver.0.1.0 初版

