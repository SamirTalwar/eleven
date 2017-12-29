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

RunningProcess = Struct.new(:name, :socket, :pid) do
  def initialize(name:, socket:, pid:)
    self.name = name
    self.socket = socket
    self.pid = pid
  end

  def is_alive?
    Process.kill 0, pid
    true
  rescue Errno::ESRCH
    info "Process \"#{name}\" has died."
    false
  end
end

class App
  def initialize(app_file:, overrides:, detach:, pid_file:)
    @app_file = app_file
    @overrides = overrides
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
    debug "Overrides: #{@overrides.inspect}"
    debug "Directory: #{@dir}"

    processes = configure()
    debug "Processes: #{processes.pretty_inspect}"
    debug

    prepare processes

    @running = true
    running_processes = start processes

    if @detach
      @forked = fork
      if @forked
        if @pid_file
          @pid_file.write("#{@forked}\n")
        end
        exit
      end
    end

    begin
      until running_processes.empty?
        running_processes.select!(&:is_alive?)
        sleep 1
      end
    rescue Interrupt
    ensure
      stop running_processes
    end
  ensure
    tear_down unless @forked
  end

  def configure
    configuration = YAML.load(@app_file.read)
    merge_overrides @overrides, configuration

    sockets = {}
    configuration['processes'].each do |name, process|
      if process['socket'] != false
        sockets[name] = @socket_dir + "#{name}.sock"
      end
    end

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
        pid = Process.spawn(*process.run_command, process.socket.to_s, config_file.to_s,
                            :in => :in, :out => :out, :err => :err,
                            :chdir => process.directory)
        started << RunningProcess.new(name: process.name, socket: process.socket, pid: pid)
      rescue StandardError => error
        info "Error spawning #{process.name}. #{error.class}: #{error.message}"
      end
    end

    started.each do |process|
      next unless process.socket
      until process.socket.exist?
        p process, process.is_alive?
        break unless process.is_alive?
        sleep 1
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

  def merge_overrides(overrides, configuration)
    overrides.each do |name, value|
      merge_override(name.split('.'), value, configuration)
    end
  end

  def merge_override(path, value, node)
    first, *rest = path
    if rest.empty?
      node[first] = value
    else
      merge_override(rest, value, node[first])
    end
  end

  def reference_sockets(node, sockets)
    if node.is_a?(Hash)
      node.each do |key, value|
        if key == 'process'
          node[key] = sockets[value].to_s
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
    overrides: {},
    detach: false,
    pid_file: nil,
  }

  PropertyNameRegexp = /[A-Za-z0-9\-]+(?:\.[A-Za-z0-9\-]+)+/
  StringProperty = Struct.new(:name, :value)
  StringPropertyRegexp = /^(#{PropertyNameRegexp})=(.+)$/
  NumberProperty = Struct.new(:name, :value)
  NumberPropertyRegexp = /^(#{PropertyNameRegexp})=(\d+(?:\.\d+)?)$/

  OptionParser.new do |opts|
    opts.accept(StringProperty) do |property|
      match = StringPropertyRegexp.match(property)
      raise "\"#{property}\" is not a valid string property assignment." unless match
      StringProperty.new(match[1], match[2])
    end

    opts.accept(NumberProperty) do |property|
      match = NumberPropertyRegexp.match(property)
      raise "\"#{property}\" is not a valid numeric property assignment." unless match
      value = match[2] =~ /\./ ? match[2].to_f : match[2].to_i
      NumberProperty.new(match[1], value)
    end

    opts.banner = "Usage: #{$0} [options]"

    opts.on('-d', '--detach', 'Run in the background') do |detach|
      options[:detach] = detach
    end
    opts.on('--pid-file=PID_FILE', 'PID file (when detaching)') do |pid_file|
      options[:pid_file] = Pathname.new(pid_file)
    end
    opts.on('--set-string=PROPERTY', 'Sets or overrides a string configuration property', StringProperty) do |property|
      options[:overrides][property.name] = property.value
    end
    opts.on('--set-number=PROPERTY', 'Sets or overrides a numeric configuration property', NumberProperty) do |property|
      options[:overrides][property.name] = property.value
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
