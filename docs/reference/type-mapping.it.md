---
title: "I tipi Lean in Rust"
description: "Come ogni tipo Lean compare nell'API Rust generata, e la feature serde."
---

La facciata rappresenta ogni tipo Lean presente in una firma esportata come segue. I tipi del
crate `lungo` sono documentati nella sua documentazione API (`cargo doc -p lungo --open`).

## Tipi predefiniti

| Lean | Rust |
| --- | --- |
| `Nat` | `lungo::Nat` (illimitato) |
| `Int` | `lungo::Int` (illimitato) |
| `Bool` | `bool` |
| `UInt8`, `UInt16`, `UInt32`, `UInt64`, `USize` | `u8`, `u16`, `u32`, `u64`, `usize` |
| `Int8`, `Int16`, `Int32`, `Int64`, `ISize` | `i8`, `i16`, `i32`, `i64`, `isize` |
| `Float`, `Float32` | `f64`, `f32` |
| `Char` | `char` |
| `String` | `String` |
| `Unit`, `PUnit` | `()` |
| `ByteArray` | `lungo::ByteArray` |
| `FloatArray` | `lungo::FloatArray` |
| `Option α` | `Option<A>` |
| `List α` | `lungo::List<A>` |
| `Array α` | `Vec<A>` |
| `α × β` | `(A, B)` |
| `Except ε α` | `Result<A, E>` |

## Effetti

| Tipo di risultato Lean | Tipo di risultato Rust |
| --- | --- |
| `IO α` | `Result<A, lungo::IoError>` |
| `EIO ε α` | `Result<A, E>` |
| `BaseIO α` | `A` |

`IoError::message()` è la resa dell'errore da parte di Lean (`IO.Error.toString`);
`IoError::user(msg)` costruisce `IO.userError msg`.

## Funzioni

Un parametro o un risultato di tipo funzione `α₁ → … → αₙ → β` è
`lungo::LeanClosure<fn(A1, …, An) -> B>`. `call(a1, …, an)` la applica;
`LeanClosure::from_fn(f)` avvolge una funzione Rust (`Fn + Send + Sync + 'static`). Sono
supportate le arietà da 1 a 8. Le funzioni che restituiscono `IO` sono opache (vedi sotto).

## Polimorfismo

I parametri di tipo diventano parametri generici vincolati da `lungo::LeanType`. Gli argomenti
di istanza diventano parametri ordinari il cui tipo è la struttura della classe, quando è del
primo ordine.

## Tipi induttivi

Un tipo induttivo del primo ordine (i cui campi hanno tutti una rappresentazione in questa
tabella) diventa un tipo Rust con gli stessi parametri:

| Lean | Rust |
| --- | --- |
| structure | `struct` con campi nominati |
| solo costruttori senza campi | `enum` senza campi |
| più costruttori | `enum`; varianti con campi nominati quando ogni argomento ha un nome, varianti tupla altrimenti |
| un solo costruttore, non una structure | `struct`, nominata o tupla come sopra |

I campi ricorsivi sono inscatolati (`Box<T>`). I tipi derivano `Clone` e `Debug`; `PartialEq`
a meno che un campo sia una funzione o un valore opaco; `Eq` e `Hash` a meno che un campo sia
anche un float. Un tipo fornito dall'applicazione con `extern_type` interrompe queste
derivazioni per i tipi che lo contengono, e altre derivazioni si aggiungono con
`type_attribute` (vedi
[configurazione](https://machinefabric.com/lungo/docs/reference/configuration#shaping-the-generated-code)).

## serde

Con la feature `serde` del crate `lungo`, i tipi della facciata implementano `Serialize` e
`Deserialize` di `serde`, così i tipi generati possono derivarli tramite `type_attribute`:

```toml
[dependencies]
lungo = { version = "1.83.0", features = ["serde"] }
```

| Rust | Serializzato come |
| --- | --- |
| `lungo::Nat`, `lungo::Int` | una stringa decimale (`"12"`, `"-3"`), poiché i valori sono illimitati; deserializzato da una stringa del genere o da un intero |
| `lungo::List<A>` | una sequenza |
| `lungo::ByteArray` | byte |
| `lungo::FloatArray` | una sequenza di float |

I valori opachi e le chiusure non hanno una forma serializzata.

## Valori opachi

I valori di qualsiasi altro tipo (tipi dipendenti, tipi con dimostrazioni o campi a valori di
tipo, funzioni che restituiscono `IO`) sono `lungo::LeanValue<M>`, dove `M` è un tipo marcatore
in `__opaque` che prende il nome dalla costante di testa del tipo. Possono essere memorizzati,
clonati e restituiti a Lean.

## Nomi

| Lean | Rust |
| --- | --- |
| componente di namespace | modulo snake_case |
| tipo, costruttore | CamelCase |
| funzione, campo, parametro | snake_case |
| parola chiave Rust | identificatore raw (`r#type`) o suffisso `_` dove gli identificatori raw non sono ammessi |

`UInt`/`USize`/`ISize` sono parole singole nella conversione di maiuscole e minuscole
(`toUInt8` → `to_uint8`). I nomi in conflitto ricevono suffissi numerici nell'ordine dei nomi
Lean. Ogni mappatura è registrata in
[`names.json`](https://machinefabric.com/lungo/docs/reference/generated-code#namesjson).
