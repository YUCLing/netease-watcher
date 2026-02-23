use windows::{
    core::Error,
    Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
    },
};

pub fn get_process_thread_ids(process_id: u32) -> Result<Vec<u32>, Error> {
    unsafe {
        // Create a snapshot of all currently running threads
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)?;

        let mut thread_entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };

        let mut thread_ids = Vec::new();

        // Iterate through all threads in the snapshot
        if Thread32First(snapshot, &mut thread_entry).is_ok() {
            loop {
                // Check if thread belongs to our target process
                if thread_entry.th32OwnerProcessID == process_id {
                    thread_ids.push(thread_entry.th32ThreadID);
                }

                // Prepare for next iteration
                thread_entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;

                // Move to next thread entry
                if Thread32Next(snapshot, &mut thread_entry).is_err() {
                    break;
                }
            }
        }

        Ok(thread_ids)
    }
}
