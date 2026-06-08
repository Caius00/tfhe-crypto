const dbName = 'db';
const storeName = 'keys';

export function getDB(): Promise<IDBDatabase> {
    return new Promise((resolve, reject) => {
        const request = indexedDB.open(dbName, 1);

        request.onupgradeneeded = (event: any) => {
            const db = event.target.result as IDBDatabase;
            if (!db.objectStoreNames.contains(storeName)) {
                db.createObjectStore(storeName);
            }
        };

        request.onsuccess = (event: any) => {
            resolve(event.target.result as IDBDatabase);
        };

        request.onerror = (event: any) => {
            reject(event.target.error);
        };
    });
}

export function setKey(db: IDBDatabase, b64: string, key: "sk" | "ck"): Promise<void> {
    return new Promise(async (resolve, reject) => {
        try {
            const transaction = db.transaction(storeName, 'readwrite');
            const store = transaction.objectStore(storeName);

            const request = store.put(b64, key);

            request.onsuccess = () => resolve();
        } catch (err) {
            reject(err);
        }
    });
}

export function getKey(db: IDBDatabase, key: "sk" | "ck"): Promise<string | undefined> {
    return new Promise(async (resolve, reject) => {
        try {
            const transaction = db.transaction(storeName, 'readonly');
            const store = transaction.objectStore(storeName);

            const request = store.get(key);

            request.onsuccess = (event: any) => resolve(event.target.result as string);
        } catch (err) {
            reject(err);
        }
    });
}