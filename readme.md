## Template for a very very basic and messy stereokit-rust-video program
The goal of this project is to offer a solution for running Gstreamer on Meta Quest. It remains to find a solution to make decodebin3 work which will allow the use of the Quest's hardware decoders
https://gitlab.freedesktop.org/gstreamer/gstreamer/-/issues/3699

## Download the source project then this template:
* git clone --recursive https://github.com/mvvvv/StereoKit-rust/
* git clone https://github.com/mvvvv/StereoKit-rust-gstreamer/

First, check that you can launch the Stereokit-rust demos as described here https://github.com/mvvvv/StereoKit-rust/blob/master/README.md

Then, go to the Stereokit-video project and transform it to your project :
- by renaming the name, package and labels in Cargo.toml, 
- and removing the .git in order to create yours,

### If you want to test video player, you'll need to get files named sintel_trailer-480p.mkv, sintel_trailer-480p.webm  and test.mp4: 
* on your PC you have to set those files under assets/videos.
* on your headset, you have to copy those files, using adb, under the "share directory"/Android/data/com.stereokit.rust_binding_video/files/videos)

### If you want to produce a rtp stream, here is an example for linux xorg:
* `gst-launch-1.0 -vvv ximagesrc ! videoconvert ! x264enc speed-preset=superfast tune=zerolatency byte-stream=true sliced-threads=true ! rtph264pay ! udpsink host=192.168.3.5 port=5000`


## Run your project on your PC's headset :
* Make sure you have [OpenXR installed](https://www.khronos.org/openxr/) with an active runtine.
* Launch: `cargo run`

## Run your project on your PC using the [simulator](https://stereokit.net/Pages/Guides/Using-The-Simulator.html) 
* Launch: `cargo run -- --test`

If you're using VsCode you'll see two launchers in launch.json to debug the project.


## Run the project on your Android headset:
* [Build GStreamer using cerbero](https://gstreamer.freedesktop.org/download/#sources) or [download GStreamer for android](https://gstreamer.freedesktop.org/download/#android). We only need the arm64 directory (Let's say we unzip it into "../gstreamer-1.24.6/".

* On windows launch: `GSTREAMER_PATH="../gstreamer-1.24.6/arm64" PKG_CONFIG_ALLOW_CROSS=1  ./gradlew run && cmd /c logcat.cmd`
* On others launch: `GSTREAMER_PATH="../gstreamer-1.24.6/arm64" PKG_CONFIG_ALLOW_CROSS=1  ./gradlew run && sh logcat.cmd`

## Build the release versions of your project:
* Desktop : `cargo build --release`
* Android : `GSTREAMER_PATH="../gstreamer-1.24.5/arm64"  PKG_CONFIG_ALLOW_CROSS=1  ./gradlew buildRelease`

Binaries are produced under ./target/release. Apk's are under./app/build/outputs/apk/release

## Compile shaders
If you want to create your own shaders, you'll need the binary `compile_sks` of the stereokit-rust project and so you have to 'install' the project: 
* `cargo install --path <path to git directory of Stereokit-rust>`

`compile_sks` calls the stereokit binary `skshaderc` using the following configuration:
* The shaders (*.hlsl files) must be created inside the shaders_src directory inside the root directory of your project. 
* The result (*.hlsl.sks files) will be produced inside the assets/shaders directory inside the root directory of your project.

To compile the *.hlsl files, go to the root directory of your project then launch `cargo compile_sks`

## Troubleshooting
Submit bugs on the [Issues tab](https://github.com/mvvvv/StereoKit-rust/issues), and ask questions in the [Discussions tab](https://github.com/mvvvv/StereoKit-rust/discussions)!

The project <https://github.com/StereoKit/StereoKit/> will give you many useful links (Discord/Twitter/Blog)
