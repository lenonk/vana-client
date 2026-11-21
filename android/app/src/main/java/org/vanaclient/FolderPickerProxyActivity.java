package org.vanaclient;

import android.app.Activity;
import android.content.Intent;
import android.net.Uri;
import android.os.Bundle;
import android.provider.DocumentsContract;
import android.util.Log;

/**
 * Document-tree picker in a plain {@link Activity}: {@link NativeActivity} on some devices never
 * receives {@code onActivityResult} from {@code startActivityForResult}. The path is written to
 * shared prefs; {@link VanaClientNativeActivity#onResume} drains it.
 */
public class FolderPickerProxyActivity extends Activity {
    private static final String TAG = "VanaClient";
    private static final int REQUEST_TREE = 1;
    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        // A recreated instance receives the pending result; launching again drops it.
        if (savedInstanceState != null) {
            return;
        }
        try {
            Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
            String start = getIntent() != null ? getIntent().getStringExtra("startPath") : null;
            Uri initial = VanaClientNativeActivity.initialFolderUri(start);
            if (initial != null) {
                intent.putExtra(DocumentsContract.EXTRA_INITIAL_URI, initial);
            }
            intent.addFlags(
                    Intent.FLAG_GRANT_READ_URI_PERMISSION
                            | Intent.FLAG_GRANT_WRITE_URI_PERMISSION
                            | Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION
                            | Intent.FLAG_GRANT_PREFIX_URI_PERMISSION);
            startActivityForResult(intent, REQUEST_TREE);
        } catch (RuntimeException e) {
            Log.e(TAG, "folder proxy", e);
            storeResult(null);
            finish();
        }
    }

    @Override
    protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        if (requestCode != REQUEST_TREE) {
            super.onActivityResult(requestCode, resultCode, data);
            return;
        }
        Uri uri = resultCode == RESULT_OK && data != null ? data.getData() : null;
        if (uri != null) {
            try {
                getContentResolver().takePersistableUriPermission(
                        uri,
                        Intent.FLAG_GRANT_READ_URI_PERMISSION | Intent.FLAG_GRANT_WRITE_URI_PERMISSION);
            } catch (RuntimeException e) {
                Log.w(TAG, "persist tree", e);
            }
        }
        String path = uri != null ? VanaClientNativeActivity.treePathFromUri(uri) : null;
        Log.i(TAG, "folder proxy result uri=" + uri + " path=" + path);
        storeResult(path);
        finish();
    }

    private void storeResult(String path) {
        VanaClientNativeActivity.storeFolderPickResult(getApplicationContext(), path);
    }
}