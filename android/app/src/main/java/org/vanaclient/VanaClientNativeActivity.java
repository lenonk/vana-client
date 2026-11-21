package org.vanaclient;

import android.app.NativeActivity;
import android.content.Context;
import android.content.Intent;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.os.Environment;
import android.provider.DocumentsContract;
import android.provider.Settings;
import android.util.Log;
import android.view.View;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.WindowManager;

/**
 * NativeActivity for {@code libvana_client.so}: full screen, and the system screens Rust asks for (All files access,
 * the browser, the folder picker).
 */
public class VanaClientNativeActivity extends NativeActivity {
    private static final String TAG = "VanaClient";

    static {
        System.loadLibrary("c++_shared");
        System.loadLibrary("vana_client");
    }

    private static native void nativeAfterUiActivity();

    @Override
    protected void onCreate(Bundle savedInstanceState) {
        super.onCreate(savedInstanceState);
        applyImmersiveUi();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) {
            applyImmersiveUi();
        }
    }

    @Override
    protected void onResume() {
        super.onResume();
        applyImmersiveUi();
        nativeAfterUiActivity();
    }

    /** Status and navigation bars hidden; a swipe from the edge shows them for a moment. */
    private void applyImmersiveUi() {
        final Window window = getWindow();
        if (window == null) {
            return;
        }
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            window.setDecorFitsSystemWindows(false);
            final WindowInsetsController c = window.getInsetsController();
            if (c != null) {
                c.hide(WindowInsets.Type.statusBars() | WindowInsets.Type.navigationBars());
                c.setSystemBarsBehavior(WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
            }
        } else {
            window.getDecorView().setSystemUiVisibility(
                    View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                            | View.SYSTEM_UI_FLAG_FULLSCREEN
                            | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                            | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                            | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                            | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN);
        }
    }

    public boolean hasAllFilesAccess() {
        return Build.VERSION.SDK_INT < Build.VERSION_CODES.R || Environment.isExternalStorageManager();
    }

    /** Opens Settings at this app's All files access switch (else the list of apps). */
    public void requestAllFilesAccess() {
        runOnUiThread(() -> {
            try {
                Intent intent = new Intent(Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION);
                intent.setData(Uri.parse("package:" + getPackageName()));
                startActivity(intent);
            } catch (RuntimeException e) {
                try {
                    startActivity(new Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION));
                } catch (RuntimeException e2) {
                    Log.e(TAG, "All files access settings", e2);
                }
            }
        });
    }

    public void openBrowser(String url) {
        runOnUiThread(() -> {
            try {
                startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url)));
            } catch (RuntimeException e) {
                Log.e(TAG, "openBrowser", e);
            }
        });
    }

    /** Writes the picker answer where Rust reads it (`files/folder-pick.txt`). Empty file = cancelled. */
    static void storeFolderPickResult(Context context, String path) {
        java.io.File out = new java.io.File(context.getFilesDir(), "folder-pick.txt");
        java.io.File tmp = new java.io.File(context.getFilesDir(), "folder-pick.txt.tmp");
        String body = path == null ? "" : path;
        try (java.io.FileOutputStream fos = new java.io.FileOutputStream(tmp)) {
            fos.write(body.getBytes(java.nio.charset.StandardCharsets.UTF_8));
            fos.getFD().sync();
        } catch (java.io.IOException e) {
            Log.e(TAG, "folder pick write", e);
            return;
        }
        if (!tmp.renameTo(out)) {
            Log.e(TAG, "folder pick rename failed");
            return;
        }
        Log.i(TAG, "folder pick stored: '" + body + "'");
        try (java.io.FileWriter w = new java.io.FileWriter(new java.io.File(context.getFilesDir(), "picker.log"), true)) {
            w.write(body + "\n");
        } catch (java.io.IOException ignored) {
        }
    }

    static void clearFolderPickFile(Context context) {
        java.io.File out = new java.io.File(context.getFilesDir(), "folder-pick.txt");
        if (out.exists() && !out.delete()) {
            Log.w(TAG, "folder pick file not cleared");
        }
    }

    public void requestFolderPicker(String startPath) {
        runOnUiThread(() -> {
            try {
                clearFolderPickFile(this);
                Intent intent = new Intent(this, FolderPickerProxyActivity.class);
                intent.putExtra("startPath", startPath);
                startActivity(intent);
            } catch (RuntimeException e) {
                Log.e(TAG, "folder picker", e);
                storeFolderPickResult(this, null);
            }
        });
    }

    static Uri initialFolderUri(String startPath) {
        String rel = primaryRelative(startPath);
        if (rel == null || rel.isEmpty()) {
            rel = "Download";
        } else {
            int slash = rel.lastIndexOf('/');
            if (slash > 0) {
                rel = rel.substring(0, slash);
            }
        }
        try {
            return DocumentsContract.buildDocumentUri("com.android.externalstorage.documents", "primary:" + rel);
        } catch (RuntimeException e) {
            return null;
        }
    }

    private static String primaryRelative(String absPath) {
        String primary = Environment.getExternalStorageDirectory().getAbsolutePath();
        if (absPath == null || !absPath.startsWith(primary)) {
            return null;
        }
        String rel = absPath.substring(primary.length());
        return rel.startsWith("/") ? rel.substring(1) : rel;
    }

    /** The plain path of a tree on primary storage (read directly under All files access). */
    static String treePathFromUri(Uri treeUri) {
        String docId = DocumentsContract.getTreeDocumentId(treeUri);
        if (docId == null) {
            docId = DocumentsContract.getDocumentId(treeUri);
        }
        if (docId == null) {
            Log.w(TAG, "folder picker: no document id in " + treeUri);
            return null;
        }
        String decoded = Uri.decode(docId);
        if (decoded.startsWith("raw:")) {
            return decoded.substring(4);
        }
        if (!decoded.startsWith("primary:")) {
            Log.w(TAG, "folder not on primary storage: " + decoded);
            return null;
        }
        String rel = decoded.substring("primary:".length());
        return Environment.getExternalStorageDirectory().getAbsolutePath() + "/" + rel;
    }
}