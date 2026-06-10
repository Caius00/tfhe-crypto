Key-Value:
# Nutzersicht:

# Architektur Entscheidungen:
- alles sehr langsam und groß
- debugging ist scheiße
	- debugger funktioniert nicht
	- builden dauert lange
	- threading probleme/ race conditions
	- wenn mit falschen keys decrypted wird, panict es nicht sondern man bekommt müll
- lib wirkt teilweise eingeschränkt
	- FheAsciiString unterstützt kein if_then_else
	- Problem mit frontend
		- server_key keine compression unterstützt (oder ich das falsch übertrage. dann aber irreführender error)
		- Client kann CompressedCiphertextList  nicht erstellen, sondern nur CompactCiphertextList
- Um clients zu isolieren -> muss key indexen -> server weiß welche entries zu wem gehören
- Um unwissenheit beim server zu garantieren, was ge-queried wurde müssen alle values gleich "lang" sein (und keys eig. auch)
	-> Problem gute größe zu finden, um nicht speicher zu überfüllen, aber auch nicht zu klein zu sein
- Speichern in komprimierter Form (ca. Faktor 16 pro Char/ FheUint8)
- Möglichkeit String anders zu kodieren (FheUint64) -> weniger overhead, dafür etwas umständlicher 

TODO() check was tammo gemacht hat
Image:
# Nutzersicht:
- an sich nicht sinvoll da auf Handy viel schneller
- nur eine Session gleichzeitig da extrem viel RAM verbraucht werden kann
	- für image ops auf einer aktuellen Session braucht es keine berechtigungen
- Nutzer bekommt Bild erst zurück, wenn er Session beendet
	- weil Bild groß sein kann
	- wenn Nutzer SessionID verliert oder malitious ist kann er den Server blockieren
	

# Architektur Entscheidungen:
- wenig optimierungspotential (glaube ich)
	- par
	- gut ram
	- möglichst wenig clonen
	- pre processing im client = cheating, da client so viel schneller ist
- dauert super lange bei etwas größeren bildern
	- bei größeren bildern wird der ram voll, wenn man versucht das ganze bild in einer var zu speichern
		-> batch bearbeitung von kleineren teilbereichen des bildes nötig/ mehr ram haben ;)
- invert ist eventuell schneller möglich indem man so große datentypen wie möglich fürs bild nimmt und dann pro pixel ops mit bit operationen und masken machen
	- nicht möglich bei z.B. white cutoff, da dort if condition nötig ist
	