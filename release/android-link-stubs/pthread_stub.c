// Bionic provides pthread in libc; some deps still pass -lpthread on Android.
void vana_android_pthread_stub(void) {}