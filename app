#!/usr/bin/env ruby

require 'fileutils'
require 'json'
require 'optparse'
require 'pathname'
require 'socket'
require 'timeout'
require 'tmpdir'
require 'yaml'

DEBUG = ENV.include?('DEBUG')

ElevenProcess = Struct.new(:name, :directory, :config)

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

    processes, sockets = configure()
    debug "Processes: #{JSON.pretty_generate(processes)}"
    debug

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
    started = start processes, sockets

    begin
      started.each do |process|
        begin
          Process.wait(process[:pid])
        rescue Errno::ECHILD
          info "Process \"#{process[:name]}\" has died."
        end
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
      config = reference_sockets(process['config'], sockets)
      directory = @app_file.dirname + process['directory']
      ElevenProcess.new(name, directory, config)
    }
    [processes, sockets]
  end

  def start(processes, sockets)
    started = []
    processes.each do |process|
      config_file = @config_dir + "#{process.name}.config"
      config_file.open('w') do |f|
        JSON.dump(process.config, f)
      end
      Thread.new do
        begin
          command = process.directory + 'run'
          pid = Process.spawn(command.to_s, sockets[process.name], config_file.to_s,
                              :in => :close, :out => :out, :err => :err,
                              :chdir => process.directory)
          started << {name: process.name, pid: pid}
          Process.wait(pid)
        rescue StandardError => error
          info "Error spawning #{process.name}. #{error.class}: #{error.message}"
        end
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
