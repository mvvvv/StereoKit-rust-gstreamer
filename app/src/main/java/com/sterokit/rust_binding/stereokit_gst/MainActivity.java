package com.stereokit.rust_binding.stereokit_gst;

import android.os.Bundle;
import android.util.Log;
import org.freedesktop.gstreamer.GStreamer;

public class MainActivity extends android.app.NativeActivity {

    @Override
    protected void onCreate( Bundle savedInstanceState ) {
        try {
            GStreamer.init(this);
        } catch (Exception  e) {
            Log.e("StereoKitJ", "Error at init : " + e);
        }
        super.onCreate(savedInstanceState);
    }

    @Override
    protected void onDestroy( ) {
        Log.d("StereoKitJ", "!!!!onDestroy");
        super.onDestroy();
    }
    
    static {
        System.loadLibrary("openxr_loader");
        System.loadLibrary("gstreamer_android");
        System.loadLibrary("stereokit_rust_gstreamer");

    }
}