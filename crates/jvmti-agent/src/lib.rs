use jni::objects::{JClass, JString};
use jni::sys::{jint, jlong, JNI_VERSION_1_8};
use jni::JNIEnv;
use jvmti::capabilities::Capabilities;
use jvmti::native::jvmti_native::{
    jclass, jlong as JvmtiLong, jmethodID, jobject, jthread, jvmtiEnv, jvmtiEventCallbacks,
    jvmtiFrameInfo, JNIEnv as JvmtiJniEnv, JavaVM, JVMTI_ENABLE, JVMTI_EVENT_VM_OBJECT_ALLOC,
    JVMTI_VERSION,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{c_char, c_void, CStr};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
use std::sync::LazyLock;
use std::sync::Mutex;
use std::time::Instant;

static RECORDING: AtomicBool = AtomicBool::new(false);
static ALLOCATION_SNAPSHOT_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static TOTAL_SAMPLES: AtomicU64 = AtomicU64::new(0);
static SAMPLING_INTERVAL_MS: AtomicU64 = AtomicU64::new(10);
static JVMTI_ENV: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());
static CPU_CYCLES_PER_NANO_BITS: AtomicU64 = AtomicU64::new(0);
static HIGH_RESOLUTION_TIMER_START: LazyLock<Instant> = LazyLock::new(Instant::now);

#[derive(Debug, Clone)]
struct HotSpotEntry {
    class_name: String,
    method_name: String,
    method_descriptor: String,
    self_samples: u64,
    total_samples: u64,
}

static HOTSPOTS: LazyLock<Mutex<HashMap<String, HotSpotEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NATIVE_JDBC_STATS: LazyLock<Vec<NativeJdbcStats>> = LazyLock::new(|| {
    (0..12)
        .map(|_| NativeJdbcStats {
            self_duration_nano: AtomicU64::new(0),
            total_duration_nano: AtomicU64::new(0),
            invocations: AtomicU64::new(0),
        })
        .collect()
});

thread_local! {
    static NATIVE_JDBC_STACK: RefCell<Vec<(i32, i64)>> = RefCell::new(Vec::with_capacity(32));
}

struct NativeJdbcStats {
    self_duration_nano: AtomicU64,
    total_duration_nano: AtomicU64,
    invocations: AtomicU64,
}

#[derive(Clone, Debug)]
struct NativeMethodMeta {
    class_name: String,
    method_name: String,
    method_descriptor: String,
}

#[derive(Clone, Debug)]
struct NativeAllocationStats {
    path: Vec<usize>,
    method: NativeMethodMeta,
    allocated_type: String,
    allocated_size: u64,
    instance_count: u64,
}

static METHOD_META: LazyLock<Mutex<HashMap<usize, NativeMethodMeta>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NATIVE_ALLOCATIONS: LazyLock<Mutex<HashMap<String, NativeAllocationStats>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[no_mangle]
pub extern "system" fn JNI_OnLoad(vm: *mut JavaVM, _reserved: *mut std::ffi::c_void) -> jint {
    println!("[jvmti-agent-rust] Agent loaded");
    unsafe {
        match initialize_jvmti(vm) {
            Ok(()) => println!("[jvmti-agent-rust] JVMTI CPU timer enabled"),
            Err(message) => println!("[jvmti-agent-rust] JVMTI CPU timer unavailable: {message}"),
        }
    }
    initialize_thread_cycle_timer();
    JNI_VERSION_1_8
}

#[no_mangle]
pub extern "system" fn JNI_OnUnload(_vm: *mut std::ffi::c_void, _reserved: *mut std::ffi::c_void) {
    println!("[jvmti-agent-rust] Agent unloaded");
}

#[no_mangle]
pub extern "system" fn Java_com_openprofiler_agent_Agent_startRecording(
    _env: JNIEnv,
    _class: JClass,
) {
    RECORDING.store(true, Ordering::SeqCst);
    TOTAL_SAMPLES.store(0, Ordering::SeqCst);
    HOTSPOTS.lock().unwrap().clear();
    NATIVE_ALLOCATIONS.lock().unwrap().clear();
    reset_native_jdbc_hotspots();
    println!("[jvmti-agent-rust] Recording started");
}

#[no_mangle]
pub extern "system" fn Java_com_openprofiler_agent_Agent_stopRecording(
    _env: JNIEnv,
    _class: JClass,
) {
    RECORDING.store(false, Ordering::SeqCst);
    println!(
        "[jvmti-agent-rust] Recording stopped. Total samples: {}",
        TOTAL_SAMPLES.load(Ordering::SeqCst)
    );
}

#[no_mangle]
pub extern "system" fn Java_com_openprofiler_agent_Agent_isRecording(
    _env: JNIEnv,
    _class: JClass,
) -> bool {
    RECORDING.load(Ordering::SeqCst)
}

#[no_mangle]
pub extern "system" fn Java_com_openprofiler_agent_Agent_getTotalSamples(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    TOTAL_SAMPLES.load(Ordering::SeqCst) as jlong
}

#[no_mangle]
pub extern "system" fn Java_com_openprofiler_agent_Agent_getSamplingIntervalMs(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    SAMPLING_INTERVAL_MS.load(Ordering::SeqCst) as jlong
}

#[no_mangle]
pub extern "system" fn Java_com_openprofiler_agent_Agent_setSamplingIntervalMs(
    _env: JNIEnv,
    _class: JClass,
    interval_ms: jlong,
) {
    SAMPLING_INTERVAL_MS.store(interval_ms as u64, Ordering::SeqCst);
}

#[no_mangle]
pub extern "system" fn Java_com_openprofiler_agent_Agent_currentThreadCpuTimeNanos(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    current_thread_cpu_time_nanos().unwrap_or(-1) as jlong
}

#[no_mangle]
pub extern "system" fn Java_com_openprofiler_agent_Agent_highResolutionTimeNanos(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    let elapsed = HIGH_RESOLUTION_TIMER_START.elapsed().as_nanos();
    elapsed.min(jlong::MAX as u128) as jlong
}

#[no_mangle]
pub extern "system" fn Java_com_openprofiler_agent_Agent_currentThreadCpuCycleTimeNanos(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    current_thread_cpu_cycle_time_nanos().unwrap_or(-1) as jlong
}

#[no_mangle]
pub extern "system" fn Java_com_openprofiler_agent_Agent_recordJdbcProbeEnter(
    _env: JNIEnv,
    _class: JClass,
    probe_id: jint,
) {
    if !RECORDING.load(Ordering::Relaxed) || !is_valid_jdbc_probe_id(probe_id) {
        return;
    }
    if let Some(now) = current_thread_cpu_cycle_time_nanos()
        .or_else(current_thread_cpu_time_nanos)
        .or_else(|| Some(HIGH_RESOLUTION_TIMER_START.elapsed().as_nanos() as i64))
    {
        NATIVE_JDBC_STACK.with(|stack| stack.borrow_mut().push((probe_id, now)));
    }
}

#[no_mangle]
pub extern "system" fn Java_com_openprofiler_agent_Agent_recordJdbcProbeExit(
    _env: JNIEnv,
    _class: JClass,
    probe_id: jint,
) {
    if !RECORDING.load(Ordering::Relaxed) || !is_valid_jdbc_probe_id(probe_id) {
        return;
    }
    let now = match current_thread_cpu_cycle_time_nanos()
        .or_else(current_thread_cpu_time_nanos)
        .or_else(|| Some(HIGH_RESOLUTION_TIMER_START.elapsed().as_nanos() as i64))
    {
        Some(now) => now,
        None => return,
    };
    NATIVE_JDBC_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        let Some((actual_probe_id, start)) = stack.pop() else {
            return;
        };
        if actual_probe_id != probe_id {
            return;
        }
        let duration = now.saturating_sub(start).max(0) as u64;
        let stats = &NATIVE_JDBC_STATS[probe_id as usize];
        stats
            .self_duration_nano
            .fetch_add(duration, Ordering::Relaxed);
        stats
            .total_duration_nano
            .fetch_add(duration, Ordering::Relaxed);
        stats.invocations.fetch_add(1, Ordering::Relaxed);
    });
}

#[no_mangle]
pub extern "system" fn Java_com_openprofiler_agent_Agent_resetNativeJdbcHotSpots(
    _env: JNIEnv,
    _class: JClass,
) {
    reset_native_jdbc_hotspots();
}

#[no_mangle]
pub extern "system" fn Java_com_openprofiler_agent_Agent_getNativeJdbcProbeAverageDurationNanos(
    _env: JNIEnv,
    _class: JClass,
    probe_id: jint,
) -> jlong {
    if !is_valid_jdbc_probe_id(probe_id) {
        return 0;
    }
    let stats = &NATIVE_JDBC_STATS[probe_id as usize];
    let invocations = stats.invocations.load(Ordering::Relaxed);
    if invocations == 0 {
        0
    } else {
        (stats.self_duration_nano.load(Ordering::Relaxed) / invocations) as jlong
    }
}

#[no_mangle]
pub extern "system" fn Java_com_openprofiler_agent_Agent_getNativeJdbcHotSpotsJson<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> JString<'local> {
    let mut json = String::from("[");
    let mut first = true;
    for (id, stats) in NATIVE_JDBC_STATS.iter().enumerate() {
        let invocations = stats.invocations.load(Ordering::Relaxed);
        if invocations == 0 {
            continue;
        }
        if !first {
            json.push(',');
        }
        first = false;
        json.push_str(&format!(
            "{{\"id\":{},\"self_duration_nano\":{},\"total_duration_nano\":{},\"invocations\":{}}}",
            id,
            stats.self_duration_nano.load(Ordering::Relaxed),
            stats.total_duration_nano.load(Ordering::Relaxed),
            invocations
        ));
    }
    json.push(']');
    env.new_string(json).unwrap()
}

#[no_mangle]
pub extern "system" fn Java_com_openprofiler_agent_Agent_getNativeAllocationHotSpotsTsv<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> JString<'local> {
    ALLOCATION_SNAPSHOT_IN_PROGRESS.store(true, Ordering::SeqCst);
    let mut rows: Vec<NativeAllocationStats> = NATIVE_ALLOCATIONS
        .lock()
        .unwrap()
        .values()
        .cloned()
        .collect();
    rows.sort_by(|a, b| {
        a.path
            .len()
            .cmp(&b.path.len())
            .then_with(|| b.allocated_size.cmp(&a.allocated_size))
    });

    let mut id_by_path = HashMap::<String, i32>::new();
    let mut lines = Vec::new();
    for row in rows {
        let key = allocation_path_key(&row.path);
        let id = lines.len() as i32;
        let parent_id = if row.path.len() > 1 {
            let parent_key = allocation_path_key(&row.path[..row.path.len() - 1]);
            *id_by_path.get(&parent_key).unwrap_or(&-1)
        } else {
            -1
        };
        id_by_path.insert(key, id);
        lines.push(format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            id,
            parent_id,
            tsv_clean(&row.method.class_name),
            tsv_clean(&row.method.method_name),
            tsv_clean(&row.method.method_descriptor),
            tsv_clean(&row.allocated_type),
            row.allocated_size,
            row.instance_count
        ));
    }

    ALLOCATION_SNAPSHOT_IN_PROGRESS.store(false, Ordering::SeqCst);
    env.new_string(lines.join("\n")).unwrap()
}

#[no_mangle]
pub extern "system" fn Java_com_openprofiler_agent_Agent_resetNativeAllocationHotSpots(
    _env: JNIEnv,
    _class: JClass,
) {
    NATIVE_ALLOCATIONS.lock().unwrap().clear();
}

unsafe fn initialize_jvmti(vm: *mut JavaVM) -> Result<(), String> {
    if vm.is_null() {
        return Err("JavaVM pointer is null".to_string());
    }

    let mut env: *mut c_void = ptr::null_mut();
    let get_env = (**vm)
        .GetEnv
        .ok_or_else(|| "JavaVM.GetEnv is null".to_string())?;
    let result = get_env(vm, &mut env as *mut *mut c_void, JVMTI_VERSION);
    if result != 0 {
        return Err(format!("JavaVM.GetEnv returned {result}"));
    }
    if env.is_null() {
        return Err("JVMTI environment pointer is null".to_string());
    }

    let jvmti = env as *mut jvmtiEnv;
    let add_capabilities = (**jvmti)
        .AddCapabilities
        .ok_or_else(|| "JVMTI.AddCapabilities is null".to_string())?;
    let capabilities = Capabilities {
        can_get_current_thread_cpu_time: true,
        can_generate_vm_object_alloc_events: true,
        ..Default::default()
    };
    let native_capabilities = capabilities.to_native();
    let error = add_capabilities(jvmti, &native_capabilities);
    if error != 0 {
        return Err(format!("JVMTI.AddCapabilities returned {error}"));
    }

    JVMTI_ENV.store(env, Ordering::SeqCst);
    install_allocation_callback(jvmti)?;
    Ok(())
}

unsafe fn install_allocation_callback(jvmti: *mut jvmtiEnv) -> Result<(), String> {
    let set_callbacks = (**jvmti)
        .SetEventCallbacks
        .ok_or_else(|| "JVMTI.SetEventCallbacks is null".to_string())?;
    let set_event_notification_mode = (**jvmti)
        .SetEventNotificationMode
        .ok_or_else(|| "JVMTI.SetEventNotificationMode is null".to_string())?;
    let mut callbacks = jvmtiEventCallbacks::default();
    callbacks.VMObjectAlloc = Some(on_vm_object_alloc);
    let error = set_callbacks(
        jvmti,
        &callbacks,
        std::mem::size_of::<jvmtiEventCallbacks>() as jint,
    );
    if error != 0 {
        return Err(format!("JVMTI.SetEventCallbacks returned {error}"));
    }
    let error = set_event_notification_mode(
        jvmti,
        JVMTI_ENABLE,
        JVMTI_EVENT_VM_OBJECT_ALLOC,
        ptr::null_mut(),
    );
    if error != 0 {
        return Err(format!(
            "JVMTI.SetEventNotificationMode(VMObjectAlloc) returned {error}"
        ));
    }
    Ok(())
}

unsafe extern "C" fn on_vm_object_alloc(
    jvmti: *mut jvmtiEnv,
    _jni: *mut JvmtiJniEnv,
    thread: jthread,
    _object: jobject,
    object_klass: jclass,
    size: JvmtiLong,
) {
    if !RECORDING.load(Ordering::Relaxed)
        || ALLOCATION_SNAPSHOT_IN_PROGRESS.load(Ordering::Relaxed)
        || jvmti.is_null()
        || thread.is_null()
        || size <= 0
    {
        return;
    }
    let Some(frames) = allocation_stack_frames(jvmti, thread) else {
        return;
    };
    if frames.is_empty() {
        return;
    }
    let allocated_type =
        jvmti_class_name(jvmti, object_klass).unwrap_or_else(|| "<unknown>".to_string());
    let mut path = Vec::<usize>::new();
    let mut metas = Vec::<NativeMethodMeta>::new();
    for frame in frames.into_iter().take(12) {
        let method_key = frame.method as usize;
        let Some(meta) = jvmti_method_meta(jvmti, frame.method) else {
            continue;
        };
        if metas.is_empty() && should_skip_allocation_top_frame(&meta.class_name) {
            return;
        }
        path.push(method_key);
        metas.push(meta);
    }
    if path.is_empty() {
        return;
    }
    let mut allocations = NATIVE_ALLOCATIONS.lock().unwrap();
    for depth in 0..path.len() {
        let sub_path = path[..=depth].to_vec();
        let key = allocation_path_key(&sub_path);
        let method = metas[depth].clone();
        let entry = allocations
            .entry(key)
            .or_insert_with(|| NativeAllocationStats {
                path: sub_path,
                method,
                allocated_type: allocated_type.clone(),
                allocated_size: 0,
                instance_count: 0,
            });
        entry.allocated_size = entry.allocated_size.saturating_add(size as u64);
        entry.instance_count = entry.instance_count.saturating_add(1);
    }
}

fn initialize_thread_cycle_timer() {
    if let Some(cycles_per_nano) = calibrate_cpu_cycles_per_nano() {
        CPU_CYCLES_PER_NANO_BITS.store(cycles_per_nano.to_bits(), Ordering::SeqCst);
        println!(
            "[jvmti-agent-rust] Thread CPU cycle timer calibrated: {:.3} cycles/ns",
            cycles_per_nano
        );
    } else {
        println!("[jvmti-agent-rust] Thread CPU cycle timer unavailable");
    }
}

fn is_valid_jdbc_probe_id(probe_id: jint) -> bool {
    probe_id >= 0 && (probe_id as usize) < NATIVE_JDBC_STATS.len()
}

fn reset_native_jdbc_hotspots() {
    for stats in NATIVE_JDBC_STATS.iter() {
        stats.self_duration_nano.store(0, Ordering::Relaxed);
        stats.total_duration_nano.store(0, Ordering::Relaxed);
        stats.invocations.store(0, Ordering::Relaxed);
    }
}

unsafe fn allocation_stack_frames(
    jvmti: *mut jvmtiEnv,
    thread: jthread,
) -> Option<Vec<jvmtiFrameInfo>> {
    let get_stack_trace = (**jvmti).GetStackTrace?;
    let mut frames = vec![jvmtiFrameInfo::default(); 32];
    let mut count: jint = 0;
    let error = get_stack_trace(
        jvmti,
        thread,
        0,
        frames.len() as jint,
        frames.as_mut_ptr(),
        &mut count,
    );
    if error != 0 || count <= 0 {
        return None;
    }
    frames.truncate(count as usize);
    Some(frames)
}

unsafe fn jvmti_method_meta(jvmti: *mut jvmtiEnv, method: jmethodID) -> Option<NativeMethodMeta> {
    let key = method as usize;
    if let Some(meta) = METHOD_META.lock().unwrap().get(&key).cloned() {
        return Some(meta);
    }

    let get_method_name = (**jvmti).GetMethodName?;
    let get_method_declaring_class = (**jvmti).GetMethodDeclaringClass?;
    let mut name_ptr: *mut c_char = ptr::null_mut();
    let mut sig_ptr: *mut c_char = ptr::null_mut();
    let mut generic_ptr: *mut c_char = ptr::null_mut();
    if get_method_name(jvmti, method, &mut name_ptr, &mut sig_ptr, &mut generic_ptr) != 0 {
        return None;
    }
    let method_name = take_jvmti_string(jvmti, name_ptr).unwrap_or_default();
    let method_descriptor = take_jvmti_string(jvmti, sig_ptr).unwrap_or_default();
    deallocate_jvmti(jvmti, generic_ptr);

    let mut declaring_class: jclass = ptr::null_mut();
    if get_method_declaring_class(jvmti, method, &mut declaring_class) != 0 {
        return None;
    }
    let class_name = jvmti_class_name(jvmti, declaring_class).unwrap_or_default();
    let meta = NativeMethodMeta {
        class_name,
        method_name,
        method_descriptor,
    };
    METHOD_META.lock().unwrap().insert(key, meta.clone());
    Some(meta)
}

unsafe fn jvmti_class_name(jvmti: *mut jvmtiEnv, klass: jclass) -> Option<String> {
    if klass.is_null() {
        return None;
    }
    let get_class_signature = (**jvmti).GetClassSignature?;
    let mut sig_ptr: *mut c_char = ptr::null_mut();
    let mut generic_ptr: *mut c_char = ptr::null_mut();
    if get_class_signature(jvmti, klass, &mut sig_ptr, &mut generic_ptr) != 0 {
        return None;
    }
    let signature = take_jvmti_string(jvmti, sig_ptr)?;
    deallocate_jvmti(jvmti, generic_ptr);
    Some(class_name_from_signature(&signature))
}

unsafe fn take_jvmti_string(jvmti: *mut jvmtiEnv, ptr_value: *mut c_char) -> Option<String> {
    if ptr_value.is_null() {
        return None;
    }
    let value = CStr::from_ptr(ptr_value).to_string_lossy().into_owned();
    deallocate_jvmti(jvmti, ptr_value);
    Some(value)
}

unsafe fn deallocate_jvmti<T>(jvmti: *mut jvmtiEnv, ptr_value: *mut T) {
    if ptr_value.is_null() {
        return;
    }
    if let Some(deallocate) = (**jvmti).Deallocate {
        let _ = deallocate(jvmti, ptr_value as *mut u8);
    }
}

fn class_name_from_signature(signature: &str) -> String {
    let mut array_depth = 0usize;
    let mut rest = signature;
    while let Some(stripped) = rest.strip_prefix('[') {
        array_depth += 1;
        rest = stripped;
    }
    let base = if rest.starts_with('L') && rest.ends_with(';') {
        rest[1..rest.len() - 1].replace('/', ".")
    } else {
        match rest {
            "B" => "byte".to_string(),
            "C" => "char".to_string(),
            "D" => "double".to_string(),
            "F" => "float".to_string(),
            "I" => "int".to_string(),
            "J" => "long".to_string(),
            "S" => "short".to_string(),
            "Z" => "boolean".to_string(),
            other => other.replace('/', "."),
        }
    };
    if array_depth == 0 {
        base
    } else {
        format!("{}{}", base, "[]".repeat(array_depth))
    }
}

fn should_skip_allocation_top_frame(class_name: &str) -> bool {
    class_name.starts_with("com.openprofiler.agent.")
        || class_name.starts_with("com.openprofiler.protocol.")
        || class_name.starts_with("com.google.protobuf.")
        || class_name.starts_with("org.objectweb.asm.")
}

fn allocation_path_key(path: &[usize]) -> String {
    path.iter()
        .map(|id| id.to_string())
        .collect::<Vec<_>>()
        .join(">")
}

fn tsv_clean(value: &str) -> String {
    value
        .replace('\t', " ")
        .replace('\r', " ")
        .replace('\n', " ")
}

fn current_thread_cpu_time_nanos() -> Option<i64> {
    current_thread_cpu_time_nanos_jvmti().or_else(current_thread_cpu_time_nanos_os)
}

fn current_thread_cpu_time_nanos_jvmti() -> Option<i64> {
    let env = JVMTI_ENV.load(Ordering::SeqCst);
    if env.is_null() {
        return None;
    }

    unsafe {
        let jvmti = env as *mut jvmtiEnv;
        let get_current_thread_cpu_time = (**jvmti).GetCurrentThreadCpuTime?;
        let mut nanos: JvmtiLong = 0;
        let error = get_current_thread_cpu_time(jvmti, &mut nanos);
        if error == 0 {
            Some(nanos as i64)
        } else {
            None
        }
    }
}

#[cfg(windows)]
fn current_thread_cpu_cycles() -> Option<u64> {
    use windows_sys::Win32::System::Threading::GetCurrentThread;
    use windows_sys::Win32::System::WindowsProgramming::QueryThreadCycleTime;

    unsafe {
        let mut cycles = 0u64;
        if QueryThreadCycleTime(GetCurrentThread(), &mut cycles) == 0 {
            None
        } else {
            Some(cycles)
        }
    }
}

#[cfg(not(windows))]
fn current_thread_cpu_cycles() -> Option<u64> {
    None
}

fn calibrate_cpu_cycles_per_nano() -> Option<f64> {
    let start_cycles = current_thread_cpu_cycles()?;
    let start = Instant::now();
    let mut value = 0x9e37_79b9_7f4a_7c15u64;
    while start.elapsed().as_millis() < 50 {
        value = value.rotate_left(7).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        std::hint::black_box(value);
    }
    let elapsed_nanos = start.elapsed().as_nanos() as f64;
    let end_cycles = current_thread_cpu_cycles()?;
    let cycles = end_cycles.checked_sub(start_cycles)? as f64;
    if elapsed_nanos > 0.0 && cycles > 0.0 {
        Some(cycles / elapsed_nanos)
    } else {
        None
    }
}

fn current_thread_cpu_cycle_time_nanos() -> Option<i64> {
    let cycles_per_nano = f64::from_bits(CPU_CYCLES_PER_NANO_BITS.load(Ordering::SeqCst));
    if cycles_per_nano <= 0.0 {
        return None;
    }
    let cycles = current_thread_cpu_cycles()? as f64;
    let nanos = cycles / cycles_per_nano;
    if nanos.is_finite() && nanos <= i64::MAX as f64 {
        Some(nanos as i64)
    } else {
        None
    }
}

#[cfg(windows)]
fn current_thread_cpu_time_nanos_os() -> Option<i64> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentThread, GetThreadTimes};

    fn filetime_to_u64(filetime: FILETIME) -> u64 {
        ((filetime.dwHighDateTime as u64) << 32) | filetime.dwLowDateTime as u64
    }

    unsafe {
        let mut creation = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut exit = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut kernel = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let mut user = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        if GetThreadTimes(
            GetCurrentThread(),
            &mut creation,
            &mut exit,
            &mut kernel,
            &mut user,
        ) == 0
        {
            return None;
        }
        let cpu_100ns = filetime_to_u64(kernel).saturating_add(filetime_to_u64(user));
        Some(cpu_100ns.saturating_mul(100) as i64)
    }
}

#[cfg(not(windows))]
fn current_thread_cpu_time_nanos_os() -> Option<i64> {
    None
}

fn get_java_string(env: &mut JNIEnv, s: JString) -> String {
    env.get_string(&s).map(|s| s.into()).unwrap_or_default()
}

#[no_mangle]
pub extern "system" fn Java_com_openprofiler_agent_Agent_recordSample(
    mut env: JNIEnv,
    _class: JClass,
    class_name: JString,
    method_name: JString,
    method_descriptor: JString,
    is_leaf: bool,
) {
    if !RECORDING.load(Ordering::SeqCst) {
        return;
    }

    TOTAL_SAMPLES.fetch_add(1, Ordering::SeqCst);

    let cn = get_java_string(&mut env, class_name);
    let mn = get_java_string(&mut env, method_name);
    let md = get_java_string(&mut env, method_descriptor);

    let key = format!("{}::{}{}", cn, mn, md);

    let mut hotspots = HOTSPOTS.lock().unwrap();
    let entry = hotspots.entry(key).or_insert_with(|| HotSpotEntry {
        class_name: cn,
        method_name: mn,
        method_descriptor: md,
        self_samples: 0,
        total_samples: 0,
    });

    entry.total_samples += 1;
    if is_leaf {
        entry.self_samples += 1;
    }
}

#[no_mangle]
pub extern "system" fn Java_com_openprofiler_agent_Agent_getHotSpotsJson<'local>(
    env: JNIEnv<'local>,
    _class: JClass<'local>,
) -> JString<'local> {
    let hotspots = HOTSPOTS.lock().unwrap();
    let total = TOTAL_SAMPLES.load(Ordering::SeqCst).max(1);

    let mut entries: Vec<_> = hotspots.values().collect();
    entries.sort_by(|a, b| b.self_samples.cmp(&a.self_samples));
    entries.truncate(250);

    let json_entries: Vec<serde_json::Value> = entries
        .iter()
        .map(|h| {
            let percent = (h.self_samples as f64 / total as f64) * 100.0;
            serde_json::json!({
                "class_name": h.class_name,
                "method_name": h.method_name,
                "method_descriptor": h.method_descriptor,
                "self_samples": h.self_samples,
                "total_samples": h.total_samples,
                "percent": percent,
            })
        })
        .collect();

    let json = serde_json::json!({
        "total_samples": total,
        "sampling_interval_ms": SAMPLING_INTERVAL_MS.load(Ordering::SeqCst),
        "hot_spots": json_entries,
    });

    let json_str = json.to_string();
    env.new_string(json_str).unwrap()
}
