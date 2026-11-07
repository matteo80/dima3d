# jt2wrl — convertitore JT → VRML 2.0

Programma separato usato dal Gestore WRL per aprire i file `.jt`: il Gestore lo avvia senza
finestra, legge l'avanzamento da stdout e apre il `.wrl` prodotto. Se un JT danneggiato fa
terminare il convertitore, il Gestore resta aperto e mostra l'errore.

Usa [JTReader](https://github.com/svr2kos2/JTReader) (C#, licenza MIT, © svr2kos2), compilato
direttamente dai sorgenti insieme a `Program.cs`. Supporta JT 8, 9 e 10 con geometria a triangoli
(LOD tassellati); la sola geometria esatta (B-rep/XT) e i file big-endian non sono convertiti.

## Uso

```
jt2wrl <entrata.jt> <uscita.wrl>
```

Stdout: righe `PROGRESS <0..1> <testo>`, alla fine `DONE <pezzi> <triangoli> <saltati>`.
Stderr: avvisi (`Avviso: ...`) ed errori. Codice di uscita: 0 riuscito, 1 argomenti, 2 errore di
lettura, 3 nessun pezzo con triangoli.

## Compilazione

Serve l'SDK .NET 8 e, per NativeAOT, Visual Studio 2022 con gli strumenti C++ (la cartella di
`vswhere.exe`, `%ProgramFiles(x86)%\Microsoft Visual Studio\Installer`, deve essere nel PATH).

```
git clone https://github.com/svr2kos2/JTReader.git tools-src/JTReader
git -C tools-src/JTReader checkout 9845868
dotnet publish tools-src/jt2wrl/jt2wrl.csproj -c Release -o tools
del tools\jt2wrl.pdb
```

Il risultato è `tools\jt2wrl.exe`, un unico eseguibile che non richiede .NET installato. Il `.pdb`, 45 MB di simboli di debug, non va distribuito. `jt2wrl.exe` va nella
cartella `tools` accanto a `gestore-wrl.exe`, con le licenze di JTReader e SharpCompress (cartella
`licenses` del repository).
