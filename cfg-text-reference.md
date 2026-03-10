# Sorbet cfg-text フォーマットリファレンス

Sorbet の `srb tc --print=cfg-text` が出力する CFG (Control Flow Graph) テキスト形式の仕様。
Rust で cfg-text パーサーを書くための情報。

## 生成コマンド

```bash
SRB_SKIP_GEM_RBIS=1 srb tc --print=cfg-text --no-error-count 2>/dev/null
```

`SRB_SKIP_GEM_RBIS=1` は Bundler 依存の gem RBI キャッシュ生成をスキップする。
終了コードは型エラーがあると非ゼロになるが、stdout には正常に出力される。

## 全体構造

```
method ::<ClassName>#<methodName> {
  <basic blocks>
}

method ::<ClassName>#<methodName> {
  <basic blocks>
}
```

- メソッドは `method ` で始まり `}` で終わる
- メソッド間は空行で区切られる
- クラスメソッドは `<Class:ClassName>#methodName`
- イニシャライザなど特殊メソッドは `<static-init>`, `<init>` など

## メソッドヘッダ

```
method ::<QualifiedClass>#<methodName> {
```

### パターン

| パターン | 例 |
|---|---|
| インスタンスメソッド | `method ::Campaign#active? {` |
| クラスメソッド | `method ::DynamoDB::<Class:Code>#decode_counter_from_code {` |
| ネストクラス | `method ::AdminArea::CampaignsController#index {` |
| 特殊メソッド | `method ::<Class:<root>>#<static-init> {` |

## Basic Block (BB)

```
bb<N>[firstDead=<M>](<params>):
    <instructions>
    <terminator>
```

- `bb0` がエントリポイント
- `bb1` は常にデッドループ（backedge先、無限ループ用）
- `firstDead`: デッド命令の開始インデックス（-1 = なし）
- `(<params>)`: BB に渡される値（phi ノード相当）

### backedges コメント

```
# backedges
# - bb0
# - bb3
```

その BB にジャンプしてくる元の BB リスト。

## 命令 (Instructions)

全命令は4スペースインデントで始まる。形式は以下のいずれか:

### 1. cast — 型キャスト
```
<self>: Campaign = cast(<self>: NilClass, Campaign);
@selected_id$10: T.nilable(String) = cast(<castTemp>$16: NilClass, T.nilable(String));
```

### 2. alias — 定数/インスタンス変数の参照
```
@campaigns$4: T.untyped = alias <C <undeclared-field-stub>> (@campaigns)
<cfgAlias>$7: T.class_of(T) = alias <C T>
<cfgAlias>$13: T.class_of(ArgumentError) = alias <C ArgumentError>
@tree$4: T.untyped = alias @tree
```

### 3. load_arg — 引数のロード
```
slot_id: String = load_arg(slot_id)
code: String = load_arg(code)
```

### 4. arg_present — オプション引数の存在チェック
```
<argPresent>$3: T::Boolean = arg_present(at)
```

### 5. メソッド呼び出し — `receiver.method(args)`
```
<statTemp>$5: T.untyped = @project$7: T.untyped.campaigns()
@booth$8: Booth = <statTemp>$9: Booth::PrivateCollectionProxy.find(<statTemp>$13: T.untyped)
<returnMethodTemp>$2: T.untyped = <self>: AdminArea::CampaignsController.render(<hashTemp>$4: Symbol(:layout), <hashTemp>$5: String("admin_area/project_editor"))
```

形式: `<lhs>: <Type> = <recv>: <RecvType>.<method>(<arg1>: <Type1>, <arg2>: <Type2>)`

### 6. return — メソッドからの返却
```
<finalReturn>: T.noreturn = return <returnMethodTemp>$2: T.untyped
```

### 7. blockreturn — ブロックからの返却
```
<blockReturnTemp>$16: T.noreturn = blockreturn<find> <blockReturnTemp>$9: T::Boolean
```

### 8. リテラル
```
<hashTemp>$8: Symbol(:created_at) = :created_at
<hashTemp>$5: String("admin_area/project_editor") = "admin_area/project_editor"
<statTemp>$17: Integer(2) = 2
<castTemp>$8: NilClass = nil
<gotoDeadTemp>$20: TrueClass = true
```

### 9. 変数の代入（別変数から）
```
<returnMethodTemp>$2: T.untyped = <assignTemp>$2
@tenant_admin$4: T.untyped = @booth_member$5
<selfRestore>$7: DynamoDB::Code = <self>
```

### 10. get-current-exception — 例外取得
```
<exceptionValue>$3: T.nilable(Exception) = <get-current-exception>
```

### 11. loadSelf / load_yield_params — ブロック系
```
<self>: DynamoDB::Code = loadSelf(find)
<blk>$8: [String, DynamoDB::Code::Consumption] = load_yield_params(find)
_$1: String = yield_load_arg(0, <blk>$8: [String, DynamoDB::Code::Consumption])
```

### 12. Solve — ブロック呼び出し結果
```
<returnMethodTemp>$2: T.nilable([String, DynamoDB::Code::Consumption]) = Solve<<block-pre-call-temp>$6, find>
```

### 13. keep-alive — IDE 用（無視してよい）
```
keep_for_ide$5: T.untyped = <keep-alive> keep_for_ide$5
```

### 14. build-hash — ハッシュ構築（Magic 内部）
```
<hashTemp>$27: {booth_id: String} = <magic>$28: T.class_of(<Magic>).<build-hash>(<hashTemp>$29: Symbol(:booth_id), <hashTemp>$30: String)
```

### 15. expand-splat — splat展開（Magic 内部）
```
<assignTemp>$3: T.untyped = <cfgAlias>$15: T.class_of(<Magic>).<expand-splat>(<assignTemp>$2: T.untyped, <statTemp>$17: Integer(2), <statTemp>$18: Integer(0))
```

### 16. isa チェック — rescue の型マッチ
```
<isaCheckTemp>$14: T::Boolean = <cfgAlias>$13: T.class_of(ArgumentError).===(<exceptionValue>$3: Exception)
```

## ターミネータ（BB 末尾）

### 無条件ジャンプ
```
    <unconditional> -> bb1
```

### 条件分岐
```
    <argPresent>$3 -> (T::Boolean ? bb2 : bb3)
    <ifTemp>$3 -> (T.untyped ? bb2 : bb3)
    <exceptionValue>$3 -> (T.nilable(Exception) ? bb3 : bb4)
    <isaCheckTemp>$14 -> (T::Boolean ? bb7 : bb8)
```

形式: `<var> -> (<Type> ? bb<true> : bb<false>)`

### ブロック呼び出し
```
    <block-call> -> (NilClass ? bb5 : bb3)
```

## 変数の命名規則

| プレフィックス | 意味 |
|---|---|
| `<self>` | レシーバ (self) |
| `<statTemp>$N` | 一時変数 |
| `<hashTemp>$N` | ハッシュキー/値用一時変数 |
| `<cfgAlias>$N` | 定数参照用一時変数 |
| `<returnMethodTemp>$N` | メソッド返り値 |
| `<blockReturnTemp>$N` | ブロック返り値 |
| `<finalReturn>` | 最終返却（常に `T.noreturn`） |
| `<assignTemp>$N` | 代入一時変数 |
| `<ifTemp>$N` | 条件分岐用 |
| `<exceptionValue>$N` | 例外値 |
| `<castTemp>$N` | キャスト用 |
| `<argPresent>$N` | オプション引数存在フラグ |
| `<selfRestore>$N` | ブロック後の self 復帰 |
| `<block-pre-call-temp>$N` | ブロック呼び出し前の一時変数 |
| `<gotoDeadTemp>$N` | dead code 到達用 |
| `<keep-alive>` | IDE 用（無視可） |
| `@name$N` | インスタンス変数 |
| `name$N` | ローカル変数 |
| `keep_for_ide$N` | IDE ヒント用（無視可） |

## 型の表記

| 表記 | 意味 |
|---|---|
| `T.untyped` | 型なし |
| `T.noreturn` | 返らない |
| `T.nilable(X)` | X または nil |
| `T.any(X, Y)` | ユニオン型 |
| `T::Boolean` | true/false |
| `T::Array[X]` | 配列 |
| `T::Hash[K, V]` | ハッシュ |
| `T.class_of(X)` | クラスオブジェクト |
| `Integer(2)` | リテラル型 |
| `Symbol(:name)` | シンボルリテラル型 |
| `String("...")` | 文字列リテラル型 |
| `NilClass` | nil |
| `TrueClass` / `FalseClass` | bool リテラル型 |
| `[X, Y]` | タプル型 |
| `{key: Type}` | Shape (typed hash) |

## 統計情報 (c5n プロジェクト)

- 総行数: 88,103
- メソッド数: 1,629
- BB ヘッダ: 9,069
- 命令行: 46,191
- 出力サイズ: 4.7 MB

### 命令の種別分布

| 命令 | 件数 |
|---|---|
| メソッド呼び出し | 9,770 |
| ブランチ（ターミネータ） | 8,777 |
| alias | 7,131 |
| シンボルリテラル | 3,433 |
| cast | 1,905 |
| return | 1,809 |
| 変数代入 | 1,722 |
| 文字列リテラル | 1,185 |
| self 参照 | 1,120 |
| blockreturn | 1,025 |
| Solve | 1,008 |
| loadSelf | 1,008 |
| bool/nil リテラル | 643 |
| load_arg | 395 |
| 数値リテラル | 376 |
| load_yield_params | 246 |
| 論理演算 | 166 |
| arg_present | 58 |
| get-current-exception | 52 |

## サンプル: シンプルなメソッド

```
method ::AdminArea::CampaignsController#edit {

bb0[firstDead=5]():
    <self>: AdminArea::CampaignsController = cast(<self>: NilClass, AdminArea::CampaignsController);
    <hashTemp>$4: Symbol(:layout) = :layout
    <hashTemp>$5: String("admin_area/project_editor") = "admin_area/project_editor"
    <returnMethodTemp>$2: T.untyped = <self>: AdminArea::CampaignsController.render(<hashTemp>$4: Symbol(:layout), <hashTemp>$5: String("admin_area/project_editor"))
    <finalReturn>: T.noreturn = return <returnMethodTemp>$2: T.untyped
    <unconditional> -> bb1

# backedges
# - bb0
bb1[firstDead=-1]():
    <unconditional> -> bb1

}
```

## サンプル: 条件分岐

```
method ::OwnerArea::BoothMembersController#index {

bb0[firstDead=-1]():
    @booth$8: T.untyped = alias <C <undeclared-field-stub>> (@booth)
    @booth_members$17: T.untyped = alias <C <undeclared-field-stub>> (@booth_members)
    <self>: OwnerArea::BoothMembersController = cast(<self>: NilClass, OwnerArea::BoothMembersController);
    <statTemp>$4: ActionController::Parameters = <self>: OwnerArea::BoothMembersController.params()
    <statTemp>$6: Symbol(:booth_id) = :booth_id
    <ifTemp>$3: T.untyped = <statTemp>$4: ActionController::Parameters.[](<statTemp>$6: Symbol(:booth_id))
    <ifTemp>$3 -> (T.untyped ? bb2 : bb3)

# backedges
# - bb4
bb1[firstDead=-1]():
    <unconditional> -> bb1

# backedges
# - bb0
bb2[firstDead=-1](<self>: OwnerArea::BoothMembersController, @booth$8: T.untyped, @booth_members$17: T.untyped):
    <cfgAlias>$12: T.class_of(Tenant) = alias <C Tenant>
    <statTemp>$10: Tenant = <cfgAlias>$12: T.class_of(Tenant).current!()
    <statTemp>$9: Booth::PrivateCollectionProxy = <statTemp>$10: Tenant.booths()
    <statTemp>$14: ActionController::Parameters = <self>: OwnerArea::BoothMembersController.params()
    <statTemp>$16: Symbol(:booth_id) = :booth_id
    <statTemp>$13: T.untyped = <statTemp>$14: ActionController::Parameters.[](<statTemp>$16: Symbol(:booth_id))
    @booth$8: Booth = <statTemp>$9: Booth::PrivateCollectionProxy.find(<statTemp>$13: T.untyped)
    @booths$9: Booth::PrivateAssociationRelation = <statTemp>$10: Booth::PrivateCollectionProxy.order(<statTemp>$14: Symbol(:name))
    <returnMethodTemp>$2: Tenant::Admin::PrivateAssociationRelation = @booth_members$17
    <unconditional> -> bb4

# backedges
# - bb0
bb3[firstDead=-1](@booth_members$17: T.untyped):
    <cfgAlias>$39: T.class_of(Tenant) = alias <C Tenant>
    <statTemp>$37: Tenant = <cfgAlias>$39: T.class_of(Tenant).current!()
    <statTemp>$36: Tenant::Admin::PrivateCollectionProxy = <statTemp>$37: Tenant.tenant_admins()
    @booth_members$17: Tenant::Admin::PrivateAssociationRelation = <statTemp>$35: Tenant::Admin::PrivateAssociationRelation.order(<hashTemp>$41: Symbol(:created_at), <hashTemp>$42: Symbol(:desc))
    <returnMethodTemp>$2: Tenant::Admin::PrivateAssociationRelation = @booth_members$17
    <unconditional> -> bb4

# backedges
# - bb2
# - bb3
bb4[firstDead=1](<returnMethodTemp>$2: Tenant::Admin::PrivateAssociationRelation):
    <finalReturn>: T.noreturn = return <returnMethodTemp>$2: Tenant::Admin::PrivateAssociationRelation
    <unconditional> -> bb1

}
```

## サンプル: ブロック渡し

```
method ::DynamoDB::Code#locked_consumption_for {

bb0[firstDead=-1]():
    <self>: DynamoDB::Code = cast(<self>: NilClass, DynamoDB::Code);
    slot_id: String = load_arg(slot_id)
    <statTemp>$3: T::Hash[String, DynamoDB::Code::Consumption] = <self>: DynamoDB::Code.consumptions_for(slot_id: String)
    <block-pre-call-temp>$6: Sorbet::Private::Static::Void = <statTemp>$3: T::Hash[String, DynamoDB::Code::Consumption].find()
    <selfRestore>$7: DynamoDB::Code = <self>
    <unconditional> -> bb2

# backedges
# - bb3
bb1[firstDead=-1]():
    <unconditional> -> bb1

# backedges
# - bb0
# - bb5
bb2[firstDead=-1](<self>: DynamoDB::Code, <block-pre-call-temp>$6: Sorbet::Private::Static::Void, <selfRestore>$7: DynamoDB::Code):
    # outerLoops: 1
    <block-call> -> (NilClass ? bb5 : bb3)

# backedges
# - bb2
bb3[firstDead=2](<block-pre-call-temp>$6: Sorbet::Private::Static::Void, <selfRestore>$7: DynamoDB::Code):
    <returnMethodTemp>$2: T.nilable([String, DynamoDB::Code::Consumption]) = Solve<<block-pre-call-temp>$6, find>
    <finalReturn>: T.noreturn = return <returnMethodTemp>$2: T.nilable([String, DynamoDB::Code::Consumption])
    <unconditional> -> bb1

# backedges
# - bb2
bb5[firstDead=9](<self>: DynamoDB::Code, <block-pre-call-temp>$6: Sorbet::Private::Static::Void, <selfRestore>$7: DynamoDB::Code):
    # outerLoops: 1
    <self>: DynamoDB::Code = loadSelf(find)
    <blk>$8: [String, DynamoDB::Code::Consumption] = load_yield_params(find)
    _$1: String = yield_load_arg(0, <blk>$8: [String, DynamoDB::Code::Consumption])
    c$1: DynamoDB::Code::Consumption = yield_load_arg(1, <blk>$8: [String, DynamoDB::Code::Consumption])
    <statTemp>$10: DynamoDB::Code::LockScope = c$1: DynamoDB::Code::Consumption.lock()
    <cfgAlias>$13: DynamoDB::Code::LockScope::NONE = alias <C NONE>
    <blockReturnTemp>$9: T::Boolean = <statTemp>$10: DynamoDB::Code::LockScope.!=(<cfgAlias>$13: DynamoDB::Code::LockScope::NONE)
    <blockReturnTemp>$16: T.noreturn = blockreturn<find> <blockReturnTemp>$9: T::Boolean
    <unconditional> -> bb2

}
```

## サンプル: 例外処理 (rescue)

```
method ::DynamoDB::<Class:Code>#decode_counter_from_code {

bb0[firstDead=-1]():
    <self>: T.class_of(DynamoDB::Code) = cast(<self>: NilClass, T.class_of(DynamoDB::Code));
    code: String = load_arg(code)
    <exceptionValue>$3: T.nilable(Exception) = <get-current-exception>
    <exceptionValue>$3 -> (T.nilable(Exception) ? bb3 : bb4)

# backedges
# - bb6
# - bb7
# - bb8
# - bb9
bb1[firstDead=-1]():
    <unconditional> -> bb1

# backedges
# - bb0
# - bb4
bb3[firstDead=-1](<returnMethodTemp>$2: T.nilable(Mangrove::Result::Ok[Integer]), <exceptionValue>$3: Exception):
    <cfgAlias>$13: T.class_of(ArgumentError) = alias <C ArgumentError>
    <isaCheckTemp>$14: T::Boolean = <cfgAlias>$13: T.class_of(ArgumentError).===(<exceptionValue>$3: Exception)
    <isaCheckTemp>$14 -> (T::Boolean ? bb7 : bb8)

# backedges
# - bb0
bb4[firstDead=-1](<self>: T.class_of(DynamoDB::Code), code: String):
    <cfgAlias>$7: T.class_of(Codegen) = alias <C Codegen>
    <statTemp>$9: T::Array[Codegen::Charset] = <self>: T.class_of(DynamoDB::Code).charsets()
    <statTemp>$5: Codegen::Decoded = <cfgAlias>$7: T.class_of(Codegen).decode(code: String, <statTemp>$9: T::Array[Codegen::Charset])
    <statTemp>$4: Integer = <statTemp>$5: Codegen::Decoded.counter()
    <returnMethodTemp>$2: Mangrove::Result::Ok[Integer] = <statTemp>$4: Integer.in_ok()
    <exceptionValue>$3: T.nilable(Exception) = <get-current-exception>
    <exceptionValue>$3 -> (T.nilable(Exception) ? bb3 : bb6)

# backedges
# - bb4
bb6[firstDead=-1](<returnMethodTemp>$2: Mangrove::Result::Ok[Integer], <gotoDeadTemp>$20: NilClass):
    <gotoDeadTemp>$20 -> (NilClass ? bb1 : bb9)

# backedges
# - bb3
bb7[firstDead=-1](<exceptionValue>$3: ArgumentError):
    <exceptionValue>$3: NilClass = nil
    <keepForCfgTemp>$11: T.untyped = <keep-alive> <exceptionValue>$3
    <cfgAlias>$16: T.class_of(Mangrove::Result::Err) = alias <C Err>
    <returnMethodTemp>$2: Mangrove::Result::Err[String] = <cfgAlias>$16: T.class_of(Mangrove::Result::Err).new(<statTemp>$19: String("不正なコードです"))
    <gotoDeadTemp>$20 -> (NilClass ? bb1 : bb9)

# backedges
# - bb3
bb8[firstDead=-1](<returnMethodTemp>$2: T.nilable(Mangrove::Result::Ok[Integer])):
    <gotoDeadTemp>$20: TrueClass = true
    <gotoDeadTemp>$20 -> (TrueClass ? bb1 : bb9)

# backedges
# - bb6
# - bb7
# - bb8
bb9[firstDead=1](<returnMethodTemp>$2: T.any(Mangrove::Result::Ok[Integer], Mangrove::Result::Err[String])):
    <finalReturn>: T.noreturn = return <returnMethodTemp>$2: T.any(Mangrove::Result::Ok[Integer], Mangrove::Result::Err[String])
    <unconditional> -> bb1

}
```

## パーサー実装のヒント (Rust)

### 推奨クレート
- `nom` or `winnow` — コンビネータパーサー
- `logos` — レキサー（トークナイザ）

### パース戦略
1. 行ベースで処理（`\n` で split）
2. 各行を先頭パターンで分類:
   - `method ` → メソッドヘッダ
   - `}` → メソッド終了
   - `bb<N>[` → BB ヘッダ
   - `# backedges` → バックエッジセクション開始
   - `# - bb<N>` → バックエッジ参照
   - `    ` (4 spaces) → 命令行
   - 空行 → スキップ
3. 命令行のパース:
   - LHS: `<name>$N: Type` or `name$N: Type` or `@name$N: Type`
   - ` = ` で分割
   - RHS を種類ごとにパース

### 注意点
- 型の中に `(` `)` `[` `]` `<` `>` がネストする（例: `T::Hash[String, DynamoDB::Code::Consumption]`）
- メソッド呼び出しの引数リストも型注釈付き
- `<static-init>` や `<Class:<root>>` は通常スキップしてよい
- `keep_for_ide` / `<keep-alive>` は IDE ヒント用、無視可
- `bb1` は常にデッドループ（`<unconditional> -> bb1` のみ）、無視可
