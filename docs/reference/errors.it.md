---
title: "Errori"
description: "Ogni codice di errore di lungo, che cosa lo causa e che cosa fare."
---

Ogni errore segnalato da lungo ha un codice stabile, stampato come `error[LNGxxxx]: …`. In
Rust, `lungo_build::Error::code()` lo restituisce come `lungo_build::ErrorCode`. Un codice
non viene mai riutilizzato per una condizione diversa.

| Intervallo | Fase |
| --- | --- |
| `LNG01xx` | Progetto e toolchain |
| `LNG02xx` | Lean |
| `LNG03xx` | Worker |
| `LNG04xx` | Extern |
| `LNG05xx` | Generazione del codice |
| `LNG06xx` | Politica di fiducia e di garanzia |
| `LNG07xx` | Registrazioni di garanzia |

Per risolvere i problemi più comuni, consulta
[Come diagnosticare una build di lungo che fallisce](https://machinefabric.com/lungo/docs/how-to/diagnose-build-failures).

## Progetto e toolchain

### LNG0101

**Progetto Lean non valido.** Il progetto Lake è incompleto o incoerente: manca
`lakefile.toml`/`lakefile.lean`, `lake-manifest.json` manca o non è valido, una dipendenza
bloccata non è stata materializzata, `lean-toolchain` è malformato, oppure la configurazione
di Lake è stata modificata durante la build.

```text
error[LNG0101]: invalid Lean project: /work/app/lean/lake-manifest.json is missing; run `lake update` once and commit the manifest
```

### LNG0102

**Toolchain Lean non supportata.** `lean-toolchain` indica una toolchain che lungo non
supporta, compresi nomi mobili come `stable`. Il messaggio elenca le toolchain supportate;
vedi [piattaforme](https://machinefabric.com/lungo/docs/reference/platforms).

### LNG0103

**Toolchain Lean non installata.** La toolchain fissata non è installata e la configurazione
non ne consente l'installazione. Installala con `elan toolchain install <toolchain>` oppure
con `lungo setup`.

### LNG0104

**Ambiente di build non supportato.** Nell'ambiente manca qualcosa di cui lungo ha bisogno
(una variabile di Cargo fuori da uno script di build, una directory home o di cache), oppure
l'ambiente non può offrire una funzionalità richiesta: la modalità `LeanOracle` in
cross-compilazione o su un target MSVC, un limite di memoria del worker dove non può essere
applicato, una toolchain il cui `lean --version` non corrisponde a quella fissata.

### LNG0105

**Errore di input/output.** Un'operazione sul file system è fallita. Il messaggio indica
l'operazione e l'errore del sistema operativo.

### LNG0106

**Comando esterno fallito.** È fallito uno strumento eseguito da lungo: `lake env`, la build
del worker stesso oppure, in modalità `LeanOracle`, la generazione C di Lake, `leanc` o
`llvm-ar`. Il messaggio contiene l'output del comando.

### LNG0107

**Configurazione non valida.** La configurazione della build è malformata o non può avere
effetto: una chiave sconosciuta o un valore errato in `lungo.toml`, un nome di modulo generato
che non è composto da lettere, cifre, `_` e `-` (compreso un nome di pacchetto Lake
inutilizzabile senza [`name`](https://machinefabric.com/lungo/docs/reference/configuration#program)),
due progetti Lean nello stesso script di build che genererebbero lo stesso modulo, nessun
`OUT_DIR` e nessun `out_dir` fuori da uno script di build, oppure un'impostazione di
[modellazione](https://machinefabric.com/lungo/docs/reference/configuration#shaping-the-generated-code)
(`type_attribute`, `struct_attribute`, `enum_attribute`, `field_attribute`, `skip_debug`,
`disable_comments`, `extern_type`) il cui percorso non seleziona nulla.

```text
error[LNG0107]: field_attribute path `Geometry.Point.z` selects no field of a generated type
```

### LNG0108

**Runtime di lungo non disponibile.** Un binding di linguaggio richiede il runtime precompilato
di lungo e nessuno è disponibile: questo `lungo` è una build di sviluppo, che non conosce
alcuna release, e non è stata indicata una distribuzione locale del runtime con
`--runtime-dir`; oppure la release non contiene un runtime per il target richiesto. Vedi
[il runtime](https://machinefabric.com/lungo/docs/reference/lungo-cli#the-runtime).

### LNG0109

**Checksum dell'artefatto del runtime non corrispondente.** Un artefatto del runtime scaricato
da `lungo runtime fetch` (o trovato nella cache) non ha il digest SHA-256 registrato in questa
release di `lungo`. L'artefatto non viene usato e la copia in cache viene rimossa. Una
discrepanza persistente significa che il download è stato manomesso o corrotto durante il
trasferimento.

### LNG0110

**Output generato non aggiornato.** `lungo generate --verify` (o `--link`) ha trovato una
directory di output i cui sorgenti generati sono diversi da quelli che il progetto genera ora;
il messaggio elenca ogni file modificato, mancante o in più. Non vengono confrontati né il
registro della build (`build-info.json`) né un prodotto di piattaforma (il `program.wasm` del
binding TypeScript). Esegui `lungo generate` e fai il commit dei sorgenti che scrive.

## Lean

### LNG0201

**Lean ha rifiutato il programma.** L'elaborazione, il controllo delle dimostrazioni da parte
del kernel o il compilatore di Lean sono falliti. Il messaggio contiene la diagnostica di Lean
con le posizioni nel sorgente.

```text
error[LNG0201]: Lean elaboration failed

error: Shapes.lean:12:75: unsolved goals
```

## Worker

### LNG0301

**Richiesta non soddisfacibile.** La configurazione chiede qualcosa che il programma non
fornisce, per esempio un export inesistente o un modulo radice fuori dal pacchetto radice del
progetto.

### LNG0302

**Output del compilatore non rappresentabile.** L'adattatore del worker per la toolchain non
riesce a codificare l'output del compilatore di Lean come Bridge IR. Indica una lacuna nel
supporto di lungo per quella toolchain.

### LNG0303

**Il worker si è interrotto.** Il processo del worker è terminato senza una risposta: è andato
in crash, è uscito o è stato terminato. Il messaggio contiene lo stato di uscita, l'output e
gli eventuali limiti di risorse in vigore.

### LNG0304

**Tempo del worker scaduto.** Il worker non ha terminato entro `worker_timeout`; il worker e
tutti i processi che aveva avviato sono stati terminati.

### LNG0305

**Limite di risorse del worker superato.** Il sistema operativo ha fermato il worker perché ha
superato `worker_cpu_limit`.

### LNG0306

**Errore di protocollo del worker.** La risposta del worker è malformata, oppure la versione del
protocollo, della Bridge IR o dell'adattatore, o il suo commit di Lean, differisce da ciò che
lungo si aspetta. Un worker in cache non aggiornato viene ricompilato automaticamente; errori di
protocollo persistenti indicano un problema di installazione.

## Extern

### LNG0401

**Simbolo extern non risolto.** Una dichiarazione `@[extern]` raggiungibile dai moduli radice
non è implementata né da una definizione Lean `@[export]` né dal runtime di lungo, e non è
un'operazione di una facility (`@[lungo_operation C]`), che solo l'host implementa; oppure è
un'operazione per cui l'output Rust non ha una mappatura `rust_extern`. Il messaggio indica la
dichiarazione, il suo simbolo, il tipo Lean, la rappresentazione richiesta e la posizione nel
sorgente.

```text
error[LNG0401]: unresolved Lean external symbol

Declaration: Unknown.providerSend
Symbol: provider_send
Lean type: Nat → Nat
Expected runtime representation: (tobj) -> tobj
Source: lean/Unknown.lean:3:1

Provide a Rust mapping with Builder::rust_extern("provider_send", "crate::path::to::function")
```

### LNG0402

**Simbolo extern implementato due volte.** Un simbolo implementato da una definizione Lean
`@[export]` è fornito anche dal runtime o da una mappatura `rust_extern`.

### LNG0403

**Simbolo del runtime rimappato.** Una mappatura `rust_extern` indica un simbolo implementato
dal runtime di lungo; le primitive del runtime non possono essere sostituite.

### LNG0404

**Rappresentazione dell'extern non corrispondente.** Le rappresentazioni dei parametri o del
risultato di una primitiva del runtime differiscono da quelle della dichiarazione Lean che ne
usa il simbolo. Indica un difetto di lungo.

### LNG0405

**Mappatura `rust_extern` inutilizzata.** Una mappatura indica un simbolo che nessuna
dichiarazione extern raggiungibile dai moduli radice utilizza.

### LNG0406

**L'extern non ha una firma Rust.** Una mappatura `rust_extern` punta a un extern il cui tipo
Lean non determina una firma di funzione Rust (per esempio uno senza una costante a livello di
sorgente). Questi extern possono essere implementati solo in Lean o dal runtime.

### LNG0407

**Forma di extern non supportata in modalità `LeanOracle`.** In modalità `LeanOracle`, le
funzioni dell'applicazione possono implementare solo extern dichiarati `@[extern "symbol"]`.

### LNG0408

**Primitiva non supportata sul target.** Il programma raggiunge una primitiva del runtime che il
target non può fornire. Su WebAssembly (il binding TypeScript) non ci sono thread, processi
figli né socket: `IO.asTask` e le altre primitive dei task, `IO.Process` e la rete non sono
disponibili. Il messaggio indica la primitiva e la dichiarazione che la usa.

## Generazione del codice

### LNG0501

**Bridge IR non valida.** Il programma viola un invariante su cui si basa il backend (scope,
arietà, rappresentazioni, letterali, completezza delle chiusure). Indica un difetto nel worker
o nel suo adattatore.

### LNG0502

**Output del compilatore non supportato.** L'output del compilatore contiene un costrutto che il
backend non implementa, come i tipi struct o union dell'IR.

### LNG0503

**Errore interno del generatore di codice.** È stato violato un invariante interno del
generatore di codice. Indica un difetto di lungo.

### LNG0504

**Plugin generatore fallito.** Un plugin generatore (`lungo-gen-<language>` nel `PATH`, vedi
[plugin](https://machinefabric.com/lungo/docs/reference/plugins)) non è stato possibile
eseguirlo, è uscito con un errore, ha scritto una risposta che non è una `GenerateResponse`
valida oppure ha segnalato errori. Il messaggio contiene il suo standard error o i suoi errori.

## Politica di fiducia e di garanzia

### LNG0601

**Violazione della politica di fiducia.** Un export, o la prova di un'affermazione su di esso,
viola `deny_sorry`, `deny_axioms` o `deny_unsafe`. Il messaggio elenca ogni violazione con le dipendenze responsabili.

```text
error[LNG0601]: exported declarations violate the configured trust policy:
  Formal.step depends on `sorry` (deny_sorry)
```

### LNG0602

**Export senza un'affermazione dimostrata.** `require-claims` (`[assurance]` di `lungo.toml`,
`Builder::require_claims`) seleziona un export che non è il soggetto di alcuna affermazione
dimostrata: nessun teorema con `@[lungo_claim … subject <export> …]` la cui prova sia priva di
`sorry`. Enunciate che cosa fa l'export e dimostratelo, oppure restringete la politica.

```text
error[LNG0602]: the export Formal.step has no proved claim (`require-claims` selects it with "."); state what it does with `@[lungo_claim]` on a theorem about it
```

### LNG0603

**Assunzione proibita.** Un'affermazione dimostrata prende come ipotesi un'assunzione che
`forbid-assumptions` nomina (o un'assunzione di una facility che nomina), oppure un export chiama
un'operazione di una facility che nomina.

## Registrazioni di garanzia

### LNG0701

**Registrazione di garanzia malformata.** Una registrazione (`decl._lungo_…`) non è nella forma
che scrive la libreria Lean di lungo, oppure nomina un tipo di specifica, una relazione, un ruolo
o un identificatore di facility che non è una stringa con spazio dei nomi ben formata, o che non è
tra quelli che lungo definisce nello spazio dei nomi `lungo`. Le registrazioni le scrivono gli
attributi `@[lungo_…]`; una scritta a mano è letta e verificata allo stesso modo.

### LNG0702

**Riferimento di garanzia pendente.** Una registrazione nomina una dichiarazione che non esiste,
un'affermazione cita una specifica senza `@[lungo_spec]`, oppure un'operazione o un'assunzione
nomina qualcosa che non è una facility.

### LNG0703

**Affermazione non valida.** La prova di un'affermazione non è un teorema, il suo soggetto è un
teorema, oppure l'enunciato della prova non ha la forma che la sua relazione `lungo.*` richiede
(per `lungo.decides`, `f … = true ↔ P …`).

### LNG0704

**Soggetto dell'affermazione assente dal suo enunciato.** L'enunciato della prova di
un'affermazione non menziona uno dei suoi soggetti o delle sue specifiche: il teorema non riguarda
ciò che l'affermazione dice.

### LNG0705

**Identificatore di garanzia duplicato.** Due facility hanno lo stesso identificatore.

### LNG0706

**Incoerenza di facility.** Un'operazione di una facility non è una dichiarazione `@[extern]` con
una voce per C, appartiene a una facility asincrona, oppure ha un simbolo che Lean (`@[export]`) o
il runtime di lungo implementano già; oppure una mappatura `rust_extern` nomina un extern che non è
un'operazione di una facility.

### LNG0707

**Interfaccia asincrona non valida.** Un export restituisce un programma asincrono
(`Lungo.Async.Program op α`) la cui istanza `Lungo.Async.Interface op` non è registrata con
`@[lungo_facility]`, le cui operazioni non possono passare all'host, o il cui tipo di risposta
dipende dagli argomenti dell'operazione; oppure un programma asincrono compare dentro un valore
invece che come risultato di una funzione.

### LNG0708

**Impronta di garanzia discordante.** `lungo assurance --compose`: due documenti di garanzia
descrivono in modo diverso una registrazione con lo stesso nome (una specifica, un'affermazione,
una facility o un'assunzione): da un altro pacchetto Lake, oppure con un altro significato. I
pacchetti sono stati generati da definizioni diverse; rigenerateli dagli stessi sorgenti Lean.

### LNG0709

**Libreria di garanzia incompatibile.** La libreria Lean di lungo
(`Lungo.Registry.schemaVersion`) scrive le registrazioni in un formato che questo lungo non legge.
Usate la libreria rilasciata con questo lungo.

### LNG0710

**Il programma dipende dall'inizializzazione di un modulo non collegato.** Il programma usa un
valore che un modulo calcola quando viene inizializzato (una dichiarazione `initialize` o
`[init]`), e lungo non collega quel modulo: è caricato solo per le sue registrazioni di garanzia
(`assurance-modules`), oppure è raggiunto solo attraverso la parte di sola compilazione della
libreria Lean di lungo (`Lungo.Attr`), quindi nulla inizializzerebbe il valore. Importate il
modulo dai moduli del programma. (L'altro codice di un tale modulo, che il compilatore di Lean può
riusare, viene eseguito così com'è.)

### LNG0711

**Documento di garanzia non valido.** Un `assurance.json` passato a `lungo assurance --compose` non
si può leggere, oppure ha una versione di schema che questo lungo non legge.
