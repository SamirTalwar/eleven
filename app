#!/usr/bin/env ruby

require 'fileutils'
require 'json'
require 'optparse'
require 'pathname'
require 'pp'
require 'socket'
require 'timeout'
require 'tmpdir'
require 'yaml'

DEBUG = ENV.include?('DEBUG')

ElevenProcess = Struct.new(:name, :directory, :prepare_command, :run_command, :socket, :config) do
  def initialize(name:, directory:, prepare_command:, run_command:, socket:, config:)
    self.name = name
    self.directory = directory
    self.prepare_command = prepare_command
    self.run_command = run_command
    self.socket = socket
    self.config = config
  end
end

class App
  def initialize(app_file:, detach:, pid_file:)
    @app_file = app_file
    @detach = detach
    @pid_file = pid_file

    @dir = Pathname.new(Dir.mktmpdir('eleven'))
    @socket_dir = @dir + 'sockets'
    @config_dir = @dir + 'config'
    @socket_dir.mkdir
    @config_dir.mkdir
  end

  def run!
    debug "Application: #{@app_file}"
    debug "Directory: #{@dir}"

    processes = configure()
    debug "Processes: #{processes.pretty_inspect}"
    debug

    prepare processes

    if @detach
      @forked = fork
      if @forked
        if @pid_file
          @pid_file.write("#{@forked}\n")
        end
        exit
      end
    end

    @running = true
    started = start processes

    begin
      running = started
      until running.empty?
        running.reject! { |process|
          begin
            Process.kill 0, process[:pid]
            false
          rescue Errno::ESRCH
            info "Process \"#{process[:name]}\" has died."
            true
          end
        }
        sleep 1
      end
    rescue Interrupt
    ensure
      stop started
    end
  ensure
    tear_down unless @forked
  end

  def configure
    configuration = YAML.load(@app_file.read)
    sockets = {}
    configuration['processes'].each { |name, process|
      sockets[name] = (@socket_dir + "#{name}.sock").to_s
    }
    processes = configuration['processes'].map { |name, process|
      directory = @app_file.dirname + process['directory']
      ElevenProcess.new(
        name: name,
        directory: directory,
        prepare_command: process['prepare'] || ((directory + 'prepare').exist? ? ['./prepare'] : []),
        run_command: process['run'] || ['./run'],
        socket: sockets[name],
        config: reference_sockets(process['config'], sockets),
      )
    }
    processes
  end

  def prepare(processes)
    processes.each do |process|
      next if process.prepare_command.empty?

      pid = Process.spawn(*process.prepare_command,
                          :in => :close, :out => :out, :err => :err,
                          :chdir => process.directory)
      Process.wait pid
      status = $?
      unless status.success?
        raise StandardError, "Process failed with an exit code of #{status.exitstatus}."
      end
    end
  end

  def start(processes)
    started = []
    processes.each do |process|
      config_file = @config_dir + "#{process.name}.config"
      config_file.open('w') do |f|
        JSON.dump(process.config, f)
      end

      begin
        pid = Process.spawn(*process.run_command, process.socket, config_file.to_s,
                            :in => :in, :out => :out, :err => :err,
                            :chdir => process.directory)
        started << {name: process.name, pid: pid}
        Thread.new do
          Process.wait(pid)
        end
      rescue StandardError => error
        info "Error spawning #{process.name}. #{error.class}: #{error.message}"
      end
    end

    started
  end

  def stop(started)
    info 'Stopping...'
    @running = false

    started.each do |process|
      pid = process[:pid]
      begin
        Process.kill 0, pid
      rescue Errno::ESRCH
        next
      end

      begin
        Process.kill 'TERM', pid
        begin
          Timeout.timeout 1 do
            Process.wait pid
          end
        rescue Errno::ECHILD
        rescue Timeout::Error
          info "Forcefully terminating #{pid}..."
          Process.kill 'KILL', pid
          begin
            Process.wait pid
          rescue Errno::ECHILD
          end
        end
      rescue StandardError => error
        info "Failed to kill PID #{pid}. #{error.class}: #{error.message}"
      end
    end

    info 'Stopped.'
  end

  def tear_down
    FileUtils.rm_r @dir
  end

  def reference_sockets(node, sockets)
    if node.is_a?(Hash)
      node.each do |key, value|
        if key == 'process'
          node[key] = sockets[value]
        else
          node[key] = reference_sockets(value, sockets)
        end
      end
    elsif node.is_a?(Array)
      node.collect { |value|
        reference_sockets(value, sockets)
      }
    else
      node
    end
  end
end

def info(*strings)
  $stderr.puts(*strings)
end

def debug(*strings)
  $stderr.puts(*strings) if DEBUG
end

if __FILE__ == $0
  options = {
    detach: false,
    pid_file: nil,
  }
  OptionParser.new do |opts|
    opts.banner = "Usage: #{$0} [options]"

    opts.on("-d", "--detach", "Run in the background") do |v|
      options[:detach] = v
    end
    opts.on("--pid-file=PID_FILE", "PID file (when detaching)") do |v|
      options[:pid_file] = Pathname.new(v)
    end
  end.parse!

  if ARGV.length != 1
    info "Usage: #{$0} CONFIGURATION-FILE"
    exit 2
  end

  app_file = Pathname.new(ARGV[0])
  unless app_file.exist?
    info "\"#{app_file}\" does not exist."
    exit 1
  end
  options[:app_file] = app_file.expand_path

  App.new(options).run!
end
