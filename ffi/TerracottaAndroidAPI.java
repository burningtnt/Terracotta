package net.burningtnt.terracotta;

import android.content.Context;
import android.net.VpnService;
import android.os.ParcelFileDescriptor;
import android.util.Log;

import androidx.annotation.Nullable;

import java.io.IOException;
import java.io.UncheckedIOException;
import java.net.InetAddress;
import java.net.UnknownHostException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.Arrays;
import java.util.Objects;
import java.util.concurrent.atomic.AtomicLong;
import java.util.concurrent.atomic.AtomicReference;

/**
 * <p>An API to handle Terracotta Android.</p>
 *
 * <h1>State Definition</h1>
 *
 * <p>For Android platform, developers must invoke {@link #initialize} with a {@link VpnServiceCallback} to initialize the rust backend.
 * Then, {@link #getState()}, {@link #setWaiting()}, {@link #setGuesting}, {@link #setScanning} are available to hook states from Terracotta.</p>
 *
 * <p>All methods here are thread-safe and can be invoked concurrently from multiple threads.</p>
 *
 * <p>For each state, self-increased {@code index} and {@code state} fields are provided.
 * A state with a greater {@code index} should be considered as a new state, while {@code state} reveals the type of the specific state.</p>
 *
 * <p>For state definitions, view <a href="https://github.com/HMCL-dev/HMCL/blob/main/HMCL/src/main/java/org/jackhuang/hmcl/terracotta/TerracottaState.java#L108-L120">all subclasses of Ready</a></p>
 *
 * <h1>VpnService</h1>
 *
 * <p>Terracotta will submit VpnService Requests when EasyTier is acquiring one.
 * To configure the callback for receiving requests, see {@link #initialize}</p>
 *
 * <p>When receiving one, {@link #getPendingVpnServiceRequest()} is available to get the pending request.
 * Developer must make sure either {@link VpnServiceRequest#startVpnService} or {@link VpnServiceRequest#reject()} is invoked,
 * or Terracotta would stuck and EasyTier cannot submit a new VpnService Request.</p>
 *
 * <p>The VpnServiceRequest must be fulfilled in 30 seconds, or it will be considered as timeout.</p>
 */
public final class TerracottaAndroidAPI {
    /**
     * <p>Callback for receiving VpnService Requests</p>
     *
     * <p>When receiving one, {@link #getPendingVpnServiceRequest()} is available to get the pending request.
     * Developer must make sure either {@link VpnServiceRequest#startVpnService} or {@link VpnServiceRequest#reject()} is invoked,
     * or Terracotta would stuck and EasyTier cannot submit a new VpnService Request.</p>
     *
     * @implNote The VpnServiceRequest must be fulfilled in 30 seconds, or it will be considered as timeout.
     */
    public interface VpnServiceCallback {
        void onStartVpnService();
    }

    /**
     * <p>A VpnService Request submitted by Terracotta. See {@link VpnServiceCallback}</p>
     */
    public interface VpnServiceRequest {
        /**
         * Create a Vpn Connection and fulfill the VpnService Request.
         *
         * @param builder A pre-configured VpnService builder.
         * @return The established Vpn Connection. Developers must close this file descriptor after EasyTier exits.
         * @throws RuntimeException if {@link VpnService.Builder#establish()} returns null.
         *
         * @implNote Developers is able to configure the builder before passing it into Terracotta
         *  to fully-custom the connection.
         */
        ParcelFileDescriptor startVpnService(VpnService.Builder builder);

        /**
         * <p>Reject the VpnServiceRequest.</p>
         */
        void reject();
    }

    static {
        System.loadLibrary("terracotta");
    }

    private static volatile VpnServiceRequest pendingRequest = null;

    private static final AtomicReference<VpnServiceCallback> VPN_SERVICE_CALLBACK = new AtomicReference<>(null);

    /**
     * <p>Get current pending VpnService Request.</p>
     * @return The pending VpnService Request.
     * @throws IllegalStateException if no pending VpnService Request exists.
     */
    public static VpnServiceRequest getPendingVpnServiceRequest() {
        VpnServiceRequest handle = pendingRequest;
        if (handle == null) {
            throw new IllegalStateException("There's no pending VpnService request.");
        }
        return handle;
    }

    /**
     * <p>Initialize the Terracotta Android.</p>
     *
     * @param context  An Android context object.
     * @param callback A callback to handle VpnService for EasyTier. See {@link VpnServiceCallback} for more information.
     */
    public static void initialize(Context context, VpnServiceCallback callback) {
        if (VPN_SERVICE_CALLBACK.compareAndSet(null, callback)) {
            Path base = context.getFilesDir().toPath().resolve("net.burningtnt.terracotta/rs");
            try {
                Files.createDirectories(base);
            } catch (IOException e) {
                throw new UncheckedIOException(e);
            }
            start0(base.toString());
        } else {
            throw new IllegalStateException("Terracotta Android has already started.");
        }
    }

    /**
     * <p>Fetch current state from Terracotta Android.</p>
     *
     * @return A json representing the current state. See {@link TerracottaAndroidAPI} for state definitions.
     * @throws IllegalStateException if Terracotta Android hasn't been initialized.
     * @implNote Usually, this method doesn't take a long time to fetch states.
     * However, when initializing the EasyTier, state fetching may block for ~1 seconds.
     */
    public static String getState() {
        assertStarted();
        return getState0();
    }

    /**
     * <p>Set Terracotta Android into 'waiting' state.</p>
     *
     * @throws IllegalStateException if Terracotta Android hasn't been initialized.
     * @implNote Usually, this method doesn't take a long time to fetch states.
     * However, when initializing the EasyTier, state fetching may block for ~1 seconds.
     */
    public static void setWaiting() {
        assertStarted();
        setWaiting0();
    }

    /**
     * <p>Set Terracotta Android into 'host-scanning' state.</p>
     *
     * @param player the player's name. A default value will be taken if it's null.
     * @throws IllegalStateException if Terracotta Android hasn't been initialized.
     * @implNote Usually, this method doesn't take a long time to fetch states.
     * However, when initializing the EasyTier, state fetching may block for ~1 seconds.
     */
    public static void setScanning(@Nullable String player) {
        assertStarted();
        setScanning0(player);
    }

    /**
     * <p>Set Terracotta Android into 'guest-connecting' state.</p>
     *
     * @param room   the room code. False will be returned if it's invalid.
     * @param player the player's name. A default value will be taken if it's null.
     * @return True if room code is valid, false otherwise.
     * @throws IllegalStateException if Terracotta Android hasn't been initialized.
     * @throws NullPointerException  if room is null.
     * @implNote Usually, this method doesn't take a long time to fetch states.
     * However, when initializing the EasyTier, state fetching may block for ~1 seconds.
     */
    public static boolean setGuesting(String room, @Nullable String player) {
        Objects.requireNonNull(room, "room");

        assertStarted();
        return setGuesting0(room, player);
    }

    private static final long FD_PENDING = ((long) Integer.MAX_VALUE) + 1;
    private static final long FD_REJECT = FD_PENDING + 1;

    private static int onVpnServiceStateChanged(byte ip1, byte ip2, byte ip3, byte ip4, short network_length, String cidr) throws UnknownHostException {
        AtomicLong fd = new AtomicLong(FD_PENDING);
        InetAddress address = InetAddress.getByAddress(new byte[]{ip1, ip2, ip3, ip4});

        pendingRequest = new VpnServiceRequest() {
            @Override
            public ParcelFileDescriptor startVpnService(VpnService.Builder builder) {
                builder.addAddress(address, network_length)
                        .addDnsServer("223.5.5.5")
                        .addDnsServer("114.114.114.114");

                if (!cidr.isEmpty()) {
                    for (String part : cidr.split("\0")) {
                        String[] parts = part.split("/", 3);
                        if (parts.length != 2) {
                            throw new IllegalArgumentException("Illegal CIDR: " + Arrays.toString(parts));
                        }
                        builder.addRoute(parts[0], Integer.parseInt(parts[1]));
                    }
                }

                ParcelFileDescriptor connection = builder.establish();
                if (connection == null) {
                    throw new RuntimeException("Cannot establish a VPN connection.");
                }

                fd.set(connection.detachFd());
                return connection;
            }

            @Override
            public void reject() {
                fd.set(FD_REJECT);
            }
        };

        VPN_SERVICE_CALLBACK.get().onStartVpnService();

        long timestamp = System.currentTimeMillis();
        while (true) {
            long value = fd.get();
            if (value == FD_PENDING) {
                if (System.currentTimeMillis() - timestamp >= 30000) {
                    Log.wtf("TerracottaAndroidAPI", "VpnService Request hasn't been fulfilled in 30s.");
                    throw new IllegalStateException();
                }
                Thread.yield();
            } else if (value == FD_REJECT) {
                pendingRequest = null;
                throw new IllegalStateException();
            } else {
                pendingRequest = null;
                return Math.toIntExact(value);
            }
        }
    }

    private static void assertStarted() {
        if (VPN_SERVICE_CALLBACK.get() == null) {
            throw new IllegalStateException("Terracotta Android hasn't started yet.");
        }
    }

    private static native void start0(String baseDir);

    private static native String getState0();

    private static native void setWaiting0();

    private static native void setScanning0(String player);

    private static native boolean setGuesting0(String room, String player);
}
