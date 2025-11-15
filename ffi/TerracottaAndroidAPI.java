package net.burningtnt.terracotta;

import android.net.VpnService;
import android.os.ParcelFileDescriptor;

import androidx.annotation.Nullable;

import java.net.InetAddress;
import java.net.UnknownHostException;
import java.util.Objects;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicReference;
import java.util.function.Function;

/**
 * <p>An API to handle Terracotta Android.</p>
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
 */
public final class TerracottaAndroidAPI {
    static {
        System.loadLibrary("terracotta");
    }

    /**
     * <p>An callback for configuring the VpnService.</p>
     *
     * <p>Developer must start a VpnService in synchrony. If an existing one exist, terminate it first.
     * In {@link VpnService#onStartCommand}, the {@code builder} callback must be invoked with a pre-configured {@link VpnService.Builder}.</p>
     *
     * <p>It's an undefined behavior if neither the {@code builder} callback is never invoked nor an exception is thrown before {@link #startVpnService} is returned.</p>
     */
    public interface VpnServiceCallback {
        void startVpnService(Function<VpnService.Builder, ParcelFileDescriptor> builder);
    }

    private static final AtomicReference<VpnServiceCallback> VPN_SERVICE_CALLBACK = new AtomicReference<>(null);

    /**
     * <p>Initialize the Terracotta Android.</p>
     *
     * @param callback A callback to handle VpnService for EasyTier. See {@link VpnServiceCallback} for more information.
     */
    public static void initialize(VpnServiceCallback callback) {
        if (VPN_SERVICE_CALLBACK.compareAndSet(null, callback)) {
            start0();
        } else {
            throw new IllegalStateException("Terracotta Android has already started.");
        }
    }

    /**
     * <p>Fetch current state from Terracotta Android.</p>
     *
     * @return A json representing the current state. See {@link TerracottaAndroidAPI} for state definitions.
     *
     * @throws IllegalStateException if Terracotta Android hasn't been initialized.
     * @implNote Usually, this method doesn't take a long time to fetch states.
     *   However, when initializing the EasyTier, state fetching may block for ~1 seconds.
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
     *   However, when initializing the EasyTier, state fetching may block for ~1 seconds.
     */
    private static void setWaiting() {
        assertStarted();
        setWaiting0();
    }

    /** <p>Set Terracotta Android into 'host-scanning' state.</p>
     *
     * @param player the player's name. A default value will be taken if it's null.
     *
     * @throws IllegalStateException if Terracotta Android hasn't been initialized.
     * @implNote Usually, this method doesn't take a long time to fetch states.
     *   However, when initializing the EasyTier, state fetching may block for ~1 seconds.
     */
    private static void setScanning(@Nullable String player) {
        assertStarted();
        setScanning0(player);
    }

    /** <p>Set Terracotta Android into 'guest-connecting' state.</p>
     *
     * @param room the room code. False will be returned if it's invalid.
     * @param player the player's name. A default value will be taken if it's null.
     *
     * @return True if room code is valid, false otherwise.
     * @throws IllegalStateException if Terracotta Android hasn't been initialized.
     * @throws NullPointerException if room is null.
     * @implNote Usually, this method doesn't take a long time to fetch states.
     *   However, when initializing the EasyTier, state fetching may block for ~1 seconds.
     */
    private static boolean setGuesting(String room, @Nullable String player) {
        Objects.requireNonNull(room, "room");

        assertStarted();
        return setGuesting0(room, player);
    }

    private static int onVpnServiceStateChanged(byte ip1, byte ip2, byte ip3, byte ip4, short network_length, String cidr) throws UnknownHostException {
        AtomicInteger fd = new AtomicInteger(0);
        InetAddress address = InetAddress.getByAddress(new byte[]{ip1, ip2, ip3, ip4});

        VPN_SERVICE_CALLBACK.get().startVpnService(builder -> {
            builder.addAddress(address, network_length)
                    .addDnsServer("223.5.5.5")
                    .addDnsServer("114.114.114.114");

            for (String part : cidr.split("\0")) {
                String[] parts = part.split("/", 3);
                if (parts.length != 2) {
                    throw new IllegalArgumentException("Illegal CIDR: " + cidr);
                }
                builder.addRoute(parts[0], Integer.parseInt(parts[1]));
            }

            ParcelFileDescriptor connection = builder.establish();
            if (connection == null) {
                throw new RuntimeException("Cannot establish a VPN connection.");
            }

            fd.set(connection.getFd());
            return connection;
        });

        while (true) {
            int value = fd.get();
            if (value != 0) {
                return value;
            }

            Thread.yield();
        }
    }

    private static void assertStarted() {
        if (VPN_SERVICE_CALLBACK.get() == null) {
            throw new IllegalStateException("Terracotta Android hasn't started yet.");
        }
    }

    private static native void start0();

    private static native String getState0();

    private static native void setWaiting0();

    private static native void setScanning0(String player);

    private static native boolean setGuesting0(String room, String player);
}
